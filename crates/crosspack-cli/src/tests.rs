#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;
    use crosspack_registry::RegistrySourceWithSnapshotStatus;
    use ed25519_dalek::{Signer, SigningKey};
    use std::sync::atomic::{AtomicU64, Ordering};

    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn begin_transaction_writes_planning_metadata_and_active_marker() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let tx = begin_transaction(
            &layout,
            "install",
            Some("git:5f1b3d8a1f2a4d0e"),
            1_771_001_234,
        )
        .expect("must start transaction");

        assert_eq!(tx.operation, "install");
        assert_eq!(tx.status, "planning");
        assert_eq!(tx.snapshot_id.as_deref(), Some("git:5f1b3d8a1f2a4d0e"));

        let active =
            std::fs::read_to_string(layout.transaction_active_path()).expect("must read active");
        assert_eq!(active.trim(), tx.txid);

        let metadata = std::fs::read_to_string(layout.transaction_metadata_path(&tx.txid))
            .expect("must read metadata");
        assert!(metadata.contains("\"status\": \"planning\""));
        assert!(metadata.contains("\"operation\": \"install\""));

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn begin_transaction_cleans_up_metadata_when_active_claim_fails() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        set_active_transaction(&layout, "tx-existing").expect("must seed existing active marker");

        let started_at_unix = 1_771_001_256;
        let expected_txid = format!("tx-{started_at_unix}-{}", std::process::id());
        let err = begin_transaction(&layout, "install", None, started_at_unix)
            .expect_err("existing active marker should block transaction start");
        assert!(
            err.to_string()
                .contains("active transaction marker already exists (txid=tx-existing)"),
            "unexpected error: {err}"
        );

        assert!(
            !layout.transaction_metadata_path(&expected_txid).exists(),
            "metadata file should be cleaned up when active claim fails"
        );
        assert!(
            !layout.transaction_staging_path(&expected_txid).exists(),
            "staging dir should be cleaned up when active claim fails"
        );

        assert_eq!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .as_deref(),
            Some("tx-existing"),
            "existing active marker should remain unchanged"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_reports_unreadable_active_marker() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        std::fs::create_dir_all(layout.transaction_active_path())
            .expect("must create unreadable active marker fixture");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("unreadable active marker should return repair-required reason");
        let expected = format!(
            "transaction state requires repair (reason=active_marker_unreadable path={})",
            layout.transaction_active_path().display()
        );
        assert!(
            err.to_string().contains(&expected),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_upgrade_command_ready_reports_preflight_context_when_transaction_active() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-blocked-upgrade-command".to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_258,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-blocked-upgrade-command")
            .expect("must write active marker");

        let err = ensure_upgrade_command_ready(&layout)
            .expect_err("active transaction should block upgrade preflight");
        assert!(
            err.to_string().contains(
                "cannot upgrade (reason=active_transaction command=upgrade): transaction tx-blocked-upgrade-command requires repair (reason=failed)"
            ),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_upgrade_command_reports_preflight_context_when_transaction_active() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-blocked-upgrade-dispatch".to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_258,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-blocked-upgrade-dispatch")
            .expect("must write active marker");

        let err = run_upgrade_command(
            &layout,
            None,
            None,
            false,
            &BTreeMap::new(),
            InstallInteractionPolicy::default(),
        )
        .expect_err("active transaction should block upgrade command");
        assert!(
            err.to_string().contains(
                "cannot upgrade (reason=active_transaction command=upgrade): transaction tx-blocked-upgrade-dispatch requires repair (reason=failed)"
            ),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_uninstall_command_reports_preflight_context_when_transaction_active() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-blocked-uninstall-command".to_string(),
            operation: "upgrade".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_259,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-blocked-uninstall-command")
            .expect("must write active marker");

        let err = run_uninstall_command(&layout, "ripgrep".to_string())
            .expect_err("active transaction should block uninstall command");
        assert!(
            err.to_string().contains(
                "cannot uninstall (reason=active_transaction command=uninstall): transaction tx-blocked-uninstall-command requires repair (reason=failed)"
            ),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_transitions_active_transaction_to_rolled_back() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-needs-rollback".to_string(),
            operation: "upgrade".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_262,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-needs-rollback").expect("must write active marker");

        let snapshot_root = layout
            .transaction_staging_path("tx-needs-rollback")
            .join("rollback")
            .join("demo");
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");
        append_transaction_journal_entry(
            &layout,
            "tx-needs-rollback",
            &TransactionJournalEntry {
                seq: 1,
                step: "backup_package_state:demo".to_string(),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup journal step");
        append_transaction_journal_entry(
            &layout,
            "tx-needs-rollback",
            &TransactionJournalEntry {
                seq: 2,
                step: "upgrade_package:demo".to_string(),
                state: "done".to_string(),
                path: Some("demo".to_string()),
            },
        )
        .expect("must append mutating journal step");

        run_rollback_command(&layout, None).expect("rollback command must succeed");

        let updated = read_transaction_metadata(&layout, "tx-needs-rollback")
            .expect("must read updated metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "rolled_back");
        assert_eq!(
            read_active_transaction(&layout).expect("must read active marker"),
            None,
            "rollback should clear active transaction marker"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_repair_command_recovers_failed_active_transaction() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-needs-repair".to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_263,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-needs-repair").expect("must write active marker");

        let snapshot_root = layout
            .transaction_staging_path("tx-needs-repair")
            .join("rollback")
            .join("demo");
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");
        append_transaction_journal_entry(
            &layout,
            "tx-needs-repair",
            &TransactionJournalEntry {
                seq: 1,
                step: "backup_package_state:demo".to_string(),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup step");
        append_transaction_journal_entry(
            &layout,
            "tx-needs-repair",
            &TransactionJournalEntry {
                seq: 2,
                step: "install_package:demo".to_string(),
                state: "done".to_string(),
                path: Some("demo".to_string()),
            },
        )
        .expect("must append mutating step");

        run_repair_command(&layout).expect("repair command must succeed");

        let updated = read_transaction_metadata(&layout, "tx-needs-repair")
            .expect("must read updated metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "rolled_back");
        assert_eq!(
            read_active_transaction(&layout).expect("must read active marker"),
            None,
            "repair should clear active marker for recovered tx"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_repair_command_recovers_active_applying_transaction() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-applying-repair".to_string(),
            operation: "install".to_string(),
            status: "applying".to_string(),
            started_at_unix: 1_771_001_265,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-applying-repair").expect("must write active marker");

        let snapshot_root = layout
            .transaction_staging_path("tx-applying-repair")
            .join("rollback")
            .join("demo");
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");
        append_transaction_journal_entry(
            &layout,
            "tx-applying-repair",
            &TransactionJournalEntry {
                seq: 1,
                step: "backup_package_state:demo".to_string(),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup step");
        append_transaction_journal_entry(
            &layout,
            "tx-applying-repair",
            &TransactionJournalEntry {
                seq: 2,
                step: "install_package:demo".to_string(),
                state: "done".to_string(),
                path: Some("demo".to_string()),
            },
        )
        .expect("must append mutating step");

        run_repair_command(&layout).expect("repair must recover active applying tx");

        let updated = read_transaction_metadata(&layout, "tx-applying-repair")
            .expect("must read updated metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "rolled_back");
        assert_eq!(
            read_active_transaction(&layout).expect("must read active marker"),
            None,
            "repair should clear active marker after recovery"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_fails_when_journal_replay_required() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-needs-replay".to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_266,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-needs-replay").expect("must write active marker");

        std::fs::write(
            layout.transaction_journal_path("tx-needs-replay"),
            r#"{"seq":1,"step":"install_package:demo","state":"done"}"#,
        )
        .expect("must write journal fixture");

        let err = run_rollback_command(&layout, Some("tx-needs-replay".to_string()))
            .expect_err("rollback should fail when replay is required");
        assert!(
            err.to_string().contains("rollback failed tx-needs-replay"),
            "unexpected error: {err}"
        );

        let active = read_active_transaction(&layout).expect("must read active marker");
        assert_eq!(active.as_deref(), Some("tx-needs-replay"));
        let updated = read_transaction_metadata(&layout, "tx-needs-replay")
            .expect("must read metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "failed");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn capture_snapshot_includes_completions_gui_and_native_state() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let package_name = "demo";
        let package_version = "1.0.0";
        let package_root = layout.package_dir(package_name, package_version);
        std::fs::create_dir_all(&package_root).expect("must create package root");
        std::fs::write(package_root.join("demo"), "#!/bin/sh\n").expect("must write package bin");

        let completion_rel_path = "packages/bash/demo--demo".to_string();
        let completion_path = exposed_completion_path(&layout, &completion_rel_path)
            .expect("must resolve completion path");
        std::fs::create_dir_all(
            completion_path
                .parent()
                .expect("must have completion parent"),
        )
        .expect("must create completion parent");
        std::fs::write(&completion_path, "complete -F _demo demo\n")
            .expect("must write completion fixture");

        let gui_asset = GuiExposureAsset {
            key: "app:demo".to_string(),
            rel_path: "launchers/demo.desktop".to_string(),
        };
        let gui_path =
            gui_asset_path(&layout, &gui_asset.rel_path).expect("must resolve gui asset path");
        std::fs::create_dir_all(gui_path.parent().expect("must have gui parent"))
            .expect("must create gui parent");
        std::fs::write(&gui_path, "[Desktop Entry]\nName=Demo\n")
            .expect("must write gui asset fixture");
        write_gui_exposure_state(&layout, package_name, std::slice::from_ref(&gui_asset))
            .expect("must write gui exposure state");

        let native_record = GuiNativeRegistrationRecord {
            key: "app:demo".to_string(),
            kind: "desktop-entry".to_string(),
            path: layout
                .prefix()
                .join("native-demo.desktop")
                .display()
                .to_string(),
        };
        write_gui_native_state(&layout, package_name, std::slice::from_ref(&native_record))
            .expect("must write native sidecar state");

        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: package_name.to_string(),
                version: package_version.to_string(),
                dependencies: Vec::new(),
                target: Some("x86_64-unknown-linux-gnu".to_string()),
                artifact_url: None,
                artifact_sha256: None,
                cache_path: None,
                exposed_bins: vec!["demo".to_string()],
                exposed_completions: vec![completion_rel_path.clone()],
                snapshot_id: None,
                install_mode: InstallMode::Managed,
                install_reason: InstallReason::Root,
                install_status: "installed".to_string(),
                installed_at_unix: 1,
            },
        )
        .expect("must write install receipt");
        std::fs::write(bin_path(&layout, "demo"), "old-bin").expect("must write exposed binary");

        let snapshot_root = capture_package_state_snapshot(&layout, "tx-capture", package_name)
            .expect("must capture snapshot");
        let manifest = read_snapshot_manifest(&snapshot_root).expect("must read snapshot manifest");

        assert!(manifest.package_exists);
        assert!(manifest.receipt_exists);
        assert_eq!(manifest.bins, vec!["demo".to_string()]);
        assert_eq!(manifest.completions, vec![completion_rel_path.clone()]);
        assert_eq!(manifest.gui_assets, vec![gui_asset.clone()]);
        assert!(manifest.native_sidecar_exists);

        assert!(snapshot_bin_path(&snapshot_root, "demo").exists());
        assert!(
            snapshot_completion_path(&snapshot_root, &completion_rel_path).exists(),
            "completion file should be captured"
        );
        assert!(
            snapshot_gui_asset_path(&snapshot_root, &gui_asset.rel_path).exists(),
            "gui asset file should be captured"
        );
        assert!(
            snapshot_native_sidecar_path(&snapshot_root).exists(),
            "native sidecar state file should be captured"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_replays_compensating_steps_and_restores_filesystem_state() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-replay-filesystem";
        let metadata = TransactionMetadata {
            version: 1,
            txid: txid.to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_266,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, txid).expect("must write active marker");

        let package_name = "demo";
        let previous_pkg_file = layout
            .pkgs_dir()
            .join(package_name)
            .join("1.0.0")
            .join("old.txt");
        std::fs::create_dir_all(previous_pkg_file.parent().expect("must resolve parent"))
            .expect("must create old package path");
        std::fs::write(&previous_pkg_file, "old-state").expect("must write old package marker");

        let previous_receipt = InstallReceipt {
            name: package_name.to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["demo".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        write_install_receipt(&layout, &previous_receipt).expect("must write previous receipt");
        std::fs::write(bin_path(&layout, "demo"), "old-bin").expect("must write old binary");

        let snapshot_root = layout
            .transaction_staging_path(txid)
            .join("rollback")
            .join(package_name);
        std::fs::create_dir_all(snapshot_root.join("package").join("1.0.0"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipts");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins");
        std::fs::copy(
            layout
                .pkgs_dir()
                .join(package_name)
                .join("1.0.0")
                .join("old.txt"),
            snapshot_root.join("package").join("1.0.0").join("old.txt"),
        )
        .expect("must copy package fixture into snapshot");
        std::fs::copy(
            layout.receipt_path(package_name),
            snapshot_root
                .join("receipt")
                .join(format!("{package_name}.receipt")),
        )
        .expect("must copy receipt fixture into snapshot");
        std::fs::copy(
            bin_path(&layout, "demo"),
            snapshot_root.join("bins").join("demo"),
        )
        .expect("must copy bin fixture into snapshot");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=1\nreceipt_exists=1\nbin=demo\n",
        )
        .expect("must write snapshot manifest");

        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 1,
                step: format!("backup_package_state:{package_name}"),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup step");

        std::fs::remove_file(bin_path(&layout, "demo")).expect("must remove old binary");
        std::fs::remove_file(layout.receipt_path(package_name)).expect("must remove old receipt");
        std::fs::remove_dir_all(layout.pkgs_dir().join(package_name))
            .expect("must remove old package state");
        std::fs::create_dir_all(layout.pkgs_dir().join(package_name).join("2.0.0"))
            .expect("must create new package state");
        std::fs::write(
            layout
                .pkgs_dir()
                .join(package_name)
                .join("2.0.0")
                .join("new.txt"),
            "new-state",
        )
        .expect("must write new package marker");
        let new_receipt = InstallReceipt {
            name: package_name.to_string(),
            version: "2.0.0".to_string(),
            dependencies: Vec::new(),
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["demo".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 2,
        };
        write_install_receipt(&layout, &new_receipt).expect("must write new receipt");
        std::fs::write(bin_path(&layout, "demo"), "new-bin").expect("must write new binary");

        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 2,
                step: format!("install_package:{package_name}"),
                state: "done".to_string(),
                path: Some(package_name.to_string()),
            },
        )
        .expect("must append mutating step");

        run_rollback_command(&layout, Some(txid.to_string()))
            .expect("rollback command should replay journal and succeed");

        let updated = read_transaction_metadata(&layout, txid)
            .expect("must read updated metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "rolled_back");
        assert!(
            read_active_transaction(&layout)
                .expect("must read active marker")
                .is_none(),
            "rollback should clear active transaction marker"
        );
        assert!(
            layout
                .pkgs_dir()
                .join(package_name)
                .join("1.0.0")
                .join("old.txt")
                .exists(),
            "rollback should restore previous package tree"
        );
        assert!(
            !layout
                .pkgs_dir()
                .join(package_name)
                .join("2.0.0")
                .join("new.txt")
                .exists(),
            "rollback should remove interrupted package tree"
        );
        let restored_receipt = read_install_receipts(&layout).expect("must load receipts");
        let restored = restored_receipt
            .iter()
            .find(|receipt| receipt.name == package_name)
            .expect("previous receipt must be restored");
        assert_eq!(restored.version, "1.0.0");
        assert_eq!(
            std::fs::read_to_string(bin_path(&layout, "demo")).expect("must read restored binary"),
            "old-bin"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn rollback_replays_native_uninstall_before_managed_restore() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-native-order";
        let package_name = "demo";

        let metadata = TransactionMetadata {
            version: 1,
            txid: txid.to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_266,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, txid).expect("must set active marker");

        let old_package_root = layout.pkgs_dir().join(package_name).join("1.0.0");
        std::fs::create_dir_all(&old_package_root).expect("must create old package root");
        let restored_marker = old_package_root.join("restored.txt");
        std::fs::write(&restored_marker, "restored").expect("must write old package marker");
        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: package_name.to_string(),
                version: "1.0.0".to_string(),
                dependencies: Vec::new(),
                target: Some("x86_64-unknown-linux-gnu".to_string()),
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
        .expect("must seed old receipt");

        let snapshot_root = capture_package_state_snapshot(&layout, txid, package_name)
            .expect("must capture snapshot");

        std::fs::remove_dir_all(layout.pkgs_dir().join(package_name))
            .expect("must remove old package state");
        let current_package_root = layout.pkgs_dir().join(package_name).join("2.0.0");
        std::fs::create_dir_all(&current_package_root).expect("must create current package root");
        std::fs::write(current_package_root.join("current.txt"), "current")
            .expect("must write current package marker");
        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: package_name.to_string(),
                version: "2.0.0".to_string(),
                dependencies: Vec::new(),
                target: Some("x86_64-unknown-linux-gnu".to_string()),
                artifact_url: None,
                artifact_sha256: None,
                cache_path: None,
                exposed_bins: Vec::new(),
                exposed_completions: Vec::new(),
                snapshot_id: None,
                install_mode: InstallMode::Native,
                install_reason: InstallReason::Root,
                install_status: "installed".to_string(),
                installed_at_unix: 2,
            },
        )
        .expect("must seed current native receipt");

        let native_live_side_effect = layout.prefix().join("native-live.desktop");
        std::fs::write(&native_live_side_effect, "native").expect("must write native side effect");
        write_gui_native_state(
            &layout,
            package_name,
            &[
                GuiNativeRegistrationRecord {
                    key: "app:demo-live".to_string(),
                    kind: "desktop-entry".to_string(),
                    path: native_live_side_effect.display().to_string(),
                },
                GuiNativeRegistrationRecord {
                    key: "app:demo-restored".to_string(),
                    kind: "desktop-entry".to_string(),
                    path: restored_marker.display().to_string(),
                },
            ],
        )
        .expect("must seed native sidecar state");

        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 1,
                step: format!("backup_package_state:{package_name}"),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup step");
        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 2,
                step: format!("install_native_package:{package_name}"),
                state: "done".to_string(),
                path: Some(package_name.to_string()),
            },
        )
        .expect("must append native mutating step");

        run_rollback_command(&layout, Some(txid.to_string()))
            .expect("rollback should replay native step and restore state");

        assert!(
            !native_live_side_effect.exists(),
            "native uninstall side effects should be reversed"
        );
        assert!(
            restored_marker.exists(),
            "native uninstall must run before managed restore operations"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn rollback_native_cleanup_uses_sidecar_when_receipt_missing() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-native-no-receipt";
        let package_name = "demo";
        let metadata = TransactionMetadata {
            version: 1,
            txid: txid.to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_269,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, txid).expect("must set active marker");

        let snapshot_root = layout
            .transaction_staging_path(txid)
            .join("rollback")
            .join(package_name);
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");

        write_gui_native_state(
            &layout,
            package_name,
            &[GuiNativeRegistrationRecord {
                key: "app:demo".to_string(),
                kind: "unsupported-kind".to_string(),
                path: "/tmp/native-demo".to_string(),
            }],
        )
        .expect("must seed native sidecar state");

        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 1,
                step: format!("backup_package_state:{package_name}"),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup step");
        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 2,
                step: format!("install_native_package:{package_name}"),
                state: "done".to_string(),
                path: Some(package_name.to_string()),
            },
        )
        .expect("must append native mutating step");

        let err = run_rollback_command(&layout, Some(txid.to_string()))
            .expect_err("native sidecar cleanup should run even without receipt");
        let message = err.to_string();
        assert!(
            message.contains("rollback failed tx-native-no-receipt"),
            "unexpected error: {message}"
        );

        let updated = read_transaction_metadata(&layout, txid)
            .expect("must read rollback metadata")
            .expect("metadata should still exist");
        assert_eq!(
            updated.status, "failed",
            "native rollback failure should preserve repairable failed state"
        );
        assert!(
            layout.gui_native_state_path(package_name).exists(),
            "sidecar should remain for repair when native cleanup fails"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_repair_command_recovers_interrupted_statuses_when_rollback_possible() {
        for status in ["planning", "applying", "rolling_back", "failed"] {
            let layout = test_layout();
            layout.ensure_base_dirs().expect("must create dirs");

            let txid = format!("tx-repair-{}", status.replace('_', "-"));
            let metadata = TransactionMetadata {
                version: 1,
                txid: txid.clone(),
                operation: "install".to_string(),
                status: status.to_string(),
                started_at_unix: 1_771_001_267,
                snapshot_id: None,
            };
            write_transaction_metadata(&layout, &metadata).expect("must write metadata");
            set_active_transaction(&layout, &txid).expect("must set active marker");

            let package_name = format!("pkg-{status}");
            let snapshot_root = layout
                .transaction_staging_path(&txid)
                .join("rollback")
                .join(&package_name);
            std::fs::create_dir_all(snapshot_root.join("package"))
                .expect("must create snapshot package directory");
            std::fs::create_dir_all(snapshot_root.join("receipt"))
                .expect("must create snapshot receipt directory");
            std::fs::create_dir_all(snapshot_root.join("bins"))
                .expect("must create snapshot bins directory");
            std::fs::write(snapshot_root.join("manifest.txt"), "")
                .expect("must create placeholder snapshot manifest");

            std::fs::create_dir_all(layout.pkgs_dir().join(&package_name).join("9.9.9"))
                .expect("must create interrupted package dir");
            std::fs::write(
                layout
                    .pkgs_dir()
                    .join(&package_name)
                    .join("9.9.9")
                    .join("partial.txt"),
                "interrupted",
            )
            .expect("must write interrupted package marker");

            append_transaction_journal_entry(
                &layout,
                &txid,
                &TransactionJournalEntry {
                    seq: 1,
                    step: format!("backup_package_state:{package_name}"),
                    state: "done".to_string(),
                    path: Some(snapshot_root.display().to_string()),
                },
            )
            .expect("must append backup step");
            append_transaction_journal_entry(
                &layout,
                &txid,
                &TransactionJournalEntry {
                    seq: 2,
                    step: format!("install_package:{package_name}"),
                    state: "done".to_string(),
                    path: Some(package_name.clone()),
                },
            )
            .expect("must append interrupted step");

            run_repair_command(&layout)
                .expect("repair should recover interrupted transaction by rollback replay");

            let updated = read_transaction_metadata(&layout, &txid)
                .expect("must read updated metadata")
                .expect("metadata should exist");
            assert_eq!(updated.status, "rolled_back", "status={status}");
            assert!(
                read_active_transaction(&layout)
                    .expect("must read active transaction")
                    .is_none(),
                "status={status}: active marker should be cleared"
            );
            assert!(
                !layout
                    .pkgs_dir()
                    .join(&package_name)
                    .join("9.9.9")
                    .join("partial.txt")
                    .exists(),
                "status={status}: interrupted package state should be rolled back"
            );

            let _ = std::fs::remove_dir_all(layout.prefix());
        }
    }

    #[test]
    fn repair_handles_interrupted_native_transaction() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-repair-native";
        let package_name = "native-demo";
        let metadata = TransactionMetadata {
            version: 1,
            txid: txid.to_string(),
            operation: "install".to_string(),
            status: "applying".to_string(),
            started_at_unix: 1_771_001_268,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, txid).expect("must set active marker");

        let snapshot_root = layout
            .transaction_staging_path(txid)
            .join("rollback")
            .join(package_name);
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");

        let current_root = layout.pkgs_dir().join(package_name).join("9.9.9");
        std::fs::create_dir_all(&current_root).expect("must create interrupted package root");
        std::fs::write(current_root.join("partial.txt"), "interrupted")
            .expect("must write interrupted package marker");
        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: package_name.to_string(),
                version: "9.9.9".to_string(),
                dependencies: Vec::new(),
                target: Some("x86_64-unknown-linux-gnu".to_string()),
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
        .expect("must seed interrupted native receipt");

        let native_side_effect = layout.prefix().join("native-repair.desktop");
        std::fs::write(&native_side_effect, "native").expect("must seed native side effect");
        write_gui_native_state(
            &layout,
            package_name,
            &[GuiNativeRegistrationRecord {
                key: "app:native-demo".to_string(),
                kind: "desktop-entry".to_string(),
                path: native_side_effect.display().to_string(),
            }],
        )
        .expect("must seed native sidecar state");

        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 1,
                step: format!("backup_package_state:{package_name}"),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup step");
        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 2,
                step: format!("install_native_package:{package_name}"),
                state: "done".to_string(),
                path: Some(package_name.to_string()),
            },
        )
        .expect("must append native mutating step");

        run_repair_command(&layout).expect("repair should rollback interrupted native transaction");

        let updated = read_transaction_metadata(&layout, txid)
            .expect("must read updated metadata")
            .expect("metadata should exist");
        assert_eq!(updated.status, "rolled_back");
        assert!(
            read_active_transaction(&layout)
                .expect("must read active marker")
                .is_none(),
            "repair should clear active marker"
        );
        assert!(
            !layout.pkgs_dir().join(package_name).exists(),
            "repair should remove interrupted package tree"
        );
        assert!(
            !native_side_effect.exists(),
            "repair should replay native uninstall side effects"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_succeeds_when_failed_tx_has_no_journal_entries() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-uninstall-no-journal".to_string(),
            operation: "uninstall".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_267,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-uninstall-no-journal")
            .expect("must write active marker");

        run_rollback_command(&layout, Some("tx-uninstall-no-journal".to_string()))
            .expect("rollback should succeed when no mutating journal entries were recorded");

        let active = read_active_transaction(&layout).expect("must read active marker");
        assert!(active.is_none(), "active marker should be cleared");
        let updated = read_transaction_metadata(&layout, "tx-uninstall-no-journal")
            .expect("must read metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "rolled_back");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_removes_orphan_bins_when_no_receipt_snapshot_exists() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-install-no-receipt";
        let package_name = "demo";

        let metadata = TransactionMetadata {
            version: 1,
            txid: txid.to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_267,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, txid).expect("must write active marker");

        let snapshot_root = layout
            .transaction_staging_path(txid)
            .join("rollback")
            .join(package_name);
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");

        let install_root = layout.pkgs_dir().join(package_name).join("2.0.0");
        std::fs::create_dir_all(&install_root).expect("must create install root");
        std::fs::write(install_root.join("demo"), "new-bin").expect("must write binary payload");
        expose_binary(&layout, &install_root, "demo", "demo")
            .expect("must expose binary without receipt");
        assert!(
            bin_path(&layout, "demo").exists(),
            "binary should exist before rollback"
        );

        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 1,
                step: format!("backup_package_state:{package_name}"),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup journal step");
        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 2,
                step: format!("install_package:{package_name}"),
                state: "done".to_string(),
                path: Some(package_name.to_string()),
            },
        )
        .expect("must append mutating journal step");

        run_rollback_command(&layout, Some(txid.to_string()))
            .expect("rollback should remove orphaned binaries for unsnapshotted install");

        assert!(
            !bin_path(&layout, "demo").exists(),
            "rollback should remove stale binary entry"
        );
        assert!(
            !layout.pkgs_dir().join(package_name).exists(),
            "rollback should remove interrupted package directory"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_rejects_invalid_txid_path_components() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let err = run_rollback_command(&layout, Some("../escape".to_string()))
            .expect_err("rollback must reject invalid txid input");
        assert!(
            err.to_string().contains("invalid rollback txid"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_without_active_marker_uses_latest_non_final_transaction() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let older = TransactionMetadata {
            version: 1,
            txid: "tx-old-failed".to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_100,
            snapshot_id: None,
        };
        let newer = TransactionMetadata {
            version: 1,
            txid: "tx-new-failed".to_string(),
            operation: "upgrade".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_200,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &older).expect("must write older metadata");
        write_transaction_metadata(&layout, &newer).expect("must write newer metadata");

        let snapshot_root = layout
            .transaction_staging_path("tx-new-failed")
            .join("rollback")
            .join("demo");
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");
        append_transaction_journal_entry(
            &layout,
            "tx-new-failed",
            &TransactionJournalEntry {
                seq: 1,
                step: "backup_package_state:demo".to_string(),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup journal step");
        append_transaction_journal_entry(
            &layout,
            "tx-new-failed",
            &TransactionJournalEntry {
                seq: 2,
                step: "upgrade_package:demo".to_string(),
                state: "done".to_string(),
                path: Some("demo".to_string()),
            },
        )
        .expect("must append mutating journal step");

        run_rollback_command(&layout, None)
            .expect("rollback without active marker should use latest non-final tx");

        let updated_newer = read_transaction_metadata(&layout, "tx-new-failed")
            .expect("must read newer metadata")
            .expect("newer metadata should exist");
        assert_eq!(updated_newer.status, "rolled_back");

        let updated_older = read_transaction_metadata(&layout, "tx-old-failed")
            .expect("must read older metadata")
            .expect("older metadata should exist");
        assert_eq!(updated_older.status, "failed");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_rejects_active_applying_transaction() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = format!("tx-live-applying-{}", std::process::id());
        let metadata = TransactionMetadata {
            version: 1,
            txid: txid.clone(),
            operation: "install".to_string(),
            status: "applying".to_string(),
            started_at_unix: current_unix_timestamp().expect("must read current timestamp"),
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, &txid).expect("must write active marker");

        let snapshot_root = layout
            .transaction_staging_path(&txid)
            .join("rollback")
            .join("demo");
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");
        append_transaction_journal_entry(
            &layout,
            &txid,
            &TransactionJournalEntry {
                seq: 1,
                step: "backup_package_state:demo".to_string(),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup journal step");
        append_transaction_journal_entry(
            &layout,
            &txid,
            &TransactionJournalEntry {
                seq: 2,
                step: "install_package:demo".to_string(),
                state: "done".to_string(),
                path: Some("demo".to_string()),
            },
        )
        .expect("must append mutating journal step");

        let err = run_rollback_command(&layout, Some(txid.clone()))
            .expect_err("rollback must reject active applying transactions");
        assert!(
            err.to_string()
                .contains("cannot rollback while transaction is active (status=applying)"),
            "unexpected error: {err}"
        );

        let active = read_active_transaction(&layout).expect("must read active marker");
        assert_eq!(active.as_deref(), Some(txid.as_str()));
        let updated = read_transaction_metadata(&layout, &txid)
            .expect("must read updated metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "applying");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_allows_stale_active_applying_transaction() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-stale-applying-99999999".to_string();
        let metadata = TransactionMetadata {
            version: 1,
            txid: txid.clone(),
            operation: "install".to_string(),
            status: "applying".to_string(),
            started_at_unix: current_unix_timestamp().expect("must read current timestamp"),
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, &txid).expect("must write active marker");

        let snapshot_root = layout
            .transaction_staging_path(&txid)
            .join("rollback")
            .join("demo");
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");
        append_transaction_journal_entry(
            &layout,
            &txid,
            &TransactionJournalEntry {
                seq: 1,
                step: "backup_package_state:demo".to_string(),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup journal step");
        append_transaction_journal_entry(
            &layout,
            &txid,
            &TransactionJournalEntry {
                seq: 2,
                step: "install_package:demo".to_string(),
                state: "done".to_string(),
                path: Some("demo".to_string()),
            },
        )
        .expect("must append mutating journal step");

        run_rollback_command(&layout, Some(txid.clone()))
            .expect("rollback should recover stale active transaction");

        let active = read_active_transaction(&layout).expect("must read active marker");
        assert_eq!(active, None);
        let updated = read_transaction_metadata(&layout, &txid)
            .expect("must read updated metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "rolled_back");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn normalize_command_token_trims_lowercases_and_falls_back() {
        assert_eq!(normalize_command_token("  UnInstall  "), "uninstall");
        assert_eq!(normalize_command_token("   \t  "), "unknown");
    }

    #[test]
    fn ensure_no_active_transaction_for_includes_command_context() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-blocked".to_string(),
            operation: "upgrade".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_260,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-blocked").expect("must write active marker");

        let err = ensure_no_active_transaction_for(&layout, "uninstall")
            .expect_err("blocked transaction should include command context");
        assert!(
            err.to_string().contains(
                "cannot uninstall (reason=active_transaction command=uninstall): transaction tx-blocked requires repair (reason=failed)"
            ),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_for_normalizes_command_token() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-blocked-normalized".to_string(),
            operation: "upgrade".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_261,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-blocked-normalized").expect("must write active marker");

        let err = ensure_no_active_transaction_for(&layout, "  UnInstall  ")
            .expect_err("blocked transaction should normalize command token");
        assert!(
            err.to_string().contains(
                "cannot uninstall (reason=active_transaction command=uninstall): transaction tx-blocked-normalized requires repair (reason=failed)"
            ),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_for_uses_unknown_when_command_missing() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-blocked-empty-command".to_string(),
            operation: "upgrade".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_262,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-blocked-empty-command")
            .expect("must write active marker");

        let err = ensure_no_active_transaction_for(&layout, "   ")
            .expect_err("blocked transaction should fallback command token");
        assert!(
            err.to_string().contains(
                "cannot unknown (reason=active_transaction command=unknown): transaction tx-blocked-empty-command requires repair (reason=failed)"
            ),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_rejects_when_marker_exists() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        set_active_transaction(&layout, "tx-abc").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("active transaction must block mutating command");
        assert!(
            err.to_string()
                .contains("transaction tx-abc requires repair"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_reports_rolling_back_status_in_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-rolling-diagnostic".to_string(),
            operation: "upgrade".to_string(),
            status: "rolling_back".to_string(),
            started_at_unix: 1_771_001_700,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-rolling-diagnostic").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("rolling_back transaction should block mutation");
        assert!(
            err.to_string().contains(
                "transaction tx-rolling-diagnostic requires repair (reason=rolling_back)"
            ),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_reports_failed_reason_in_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-failed-diagnostic".to_string(),
            operation: "upgrade".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_710,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-failed-diagnostic").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("failed transaction should block mutation");
        assert!(
            err.to_string()
                .contains("transaction tx-failed-diagnostic requires repair (reason=failed)"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_reports_unreadable_metadata_in_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-corrupt-meta";
        std::fs::write(layout.transaction_metadata_path(txid), "{invalid-json")
            .expect("must write corrupt metadata");
        set_active_transaction(&layout, txid).expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("corrupt metadata should block mutating command");
        let expected = format!(
            "transaction tx-corrupt-meta requires repair (reason=metadata_unreadable path={})",
            layout.transaction_metadata_path(txid).display()
        );
        assert!(
            err.to_string().contains(&expected),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_reports_missing_metadata_in_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        set_active_transaction(&layout, "tx-missing-meta").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("missing metadata should block mutating command");
        let expected = format!(
            "transaction tx-missing-meta requires repair (reason=metadata_missing path={})",
            layout
                .transaction_metadata_path("tx-missing-meta")
                .display()
        );
        assert!(
            err.to_string().contains(&expected),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_includes_status_when_metadata_exists() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-abc".to_string(),
            operation: "install".to_string(),
            status: "paused".to_string(),
            started_at_unix: 1_771_001_300,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-abc").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("active transaction must include status context");
        assert!(
            err.to_string()
                .contains("transaction tx-abc is active (reason=active_status status=paused)"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_clears_committed_marker() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-committed".to_string(),
            operation: "install".to_string(),
            status: "committed".to_string(),
            started_at_unix: 1_771_001_360,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-committed").expect("must write active marker");

        ensure_no_active_transaction(&layout)
            .expect("committed transaction marker should be auto-cleaned");

        assert!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .is_none(),
            "committed active marker should be cleared"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_blocks_planning_without_mutating_status() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-planning".to_string(),
            operation: "install".to_string(),
            status: "planning".to_string(),
            started_at_unix: 1_771_001_420,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-planning").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("planning transaction should block concurrent mutation");
        assert!(
            err.to_string().contains(
                "transaction tx-planning is active (reason=active_status status=planning)"
            ),
            "unexpected error: {err}"
        );

        let updated = read_transaction_metadata(&layout, "tx-planning")
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(updated.status, "planning");
        assert_eq!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .as_deref(),
            Some("tx-planning"),
            "planning marker should remain active"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_clears_rolled_back_marker() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-rolled-back".to_string(),
            operation: "upgrade".to_string(),
            status: "rolled_back".to_string(),
            started_at_unix: 1_771_001_430,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-rolled-back").expect("must write active marker");

        ensure_no_active_transaction(&layout)
            .expect("rolled_back transaction marker should be auto-cleaned");

        assert!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .is_none(),
            "rolled_back active marker should be cleared"
        );

        ensure_no_active_transaction(&layout)
            .expect("cleanup path should remain idempotent after marker is removed");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn set_transaction_status_updates_metadata_via_helper() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let tx = begin_transaction(&layout, "install", None, 1_771_001_500)
            .expect("must create transaction");

        set_transaction_status(&layout, &tx.txid, "applying").expect("must update status");

        let metadata = read_transaction_metadata(&layout, &tx.txid)
            .expect("must read metadata")
            .expect("metadata must exist");
        assert_eq!(metadata.status, "applying");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn execute_with_transaction_commits_and_clears_active_marker() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut txid = None;
        execute_with_transaction(&layout, "upgrade", None, |tx| {
            txid = Some(tx.txid.clone());
            Ok(())
        })
        .expect("transaction should commit");

        let txid = txid.expect("txid should be captured");
        let metadata = read_transaction_metadata(&layout, &txid)
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(metadata.status, "committed");
        assert_eq!(metadata.operation, "upgrade");
        assert!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .is_none(),
            "active marker should be cleared"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn execute_with_transaction_marks_failed_on_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut txid = None;
        let err = execute_with_transaction(&layout, "uninstall", None, |tx| {
            txid = Some(tx.txid.clone());
            Err(anyhow::anyhow!("boom"))
        })
        .expect_err("failing transaction must return error");
        assert!(err.to_string().contains("boom"));

        let txid = txid.expect("txid should be captured");
        let metadata = read_transaction_metadata(&layout, &txid)
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(metadata.status, "failed");
        assert_eq!(metadata.operation, "uninstall");
        assert_eq!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .as_deref(),
            Some(txid.as_str()),
            "failed transaction should retain active marker for repair"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn execute_with_transaction_preserves_rolling_back_status_on_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut txid = None;
        let err = execute_with_transaction(&layout, "upgrade", None, |tx| {
            txid = Some(tx.txid.clone());
            set_transaction_status(&layout, &tx.txid, "rolling_back")?;
            Err(anyhow::anyhow!("rollback in progress"))
        })
        .expect_err("failing rollback transaction must return error");
        assert!(err.to_string().contains("rollback in progress"));

        let txid = txid.expect("txid should be captured");
        let metadata = read_transaction_metadata(&layout, &txid)
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(metadata.status, "rolling_back");
        assert_eq!(metadata.operation, "upgrade");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn execute_with_transaction_preserves_rolled_back_status_on_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut txid = None;
        let err = execute_with_transaction(&layout, "uninstall", None, |tx| {
            txid = Some(tx.txid.clone());
            set_transaction_status(&layout, &tx.txid, "rolled_back")?;
            Err(anyhow::anyhow!("post-rollback cleanup failed"))
        })
        .expect_err("rolled_back transaction should preserve status on error");
        assert!(err.to_string().contains("post-rollback cleanup failed"));

        let txid = txid.expect("txid should be captured");
        let metadata = read_transaction_metadata(&layout, &txid)
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(metadata.status, "rolled_back");
        assert_eq!(metadata.operation, "uninstall");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn execute_with_transaction_clears_active_marker_when_rolled_back_on_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut txid = None;
        let err = execute_with_transaction(&layout, "upgrade", None, |tx| {
            txid = Some(tx.txid.clone());
            set_transaction_status(&layout, &tx.txid, "rolled_back")?;
            Err(anyhow::anyhow!("cleanup warning"))
        })
        .expect_err("rolled_back error path should still return original error");
        assert!(err.to_string().contains("cleanup warning"));

        let txid = txid.expect("txid should be captured");
        let metadata = read_transaction_metadata(&layout, &txid)
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(metadata.status, "rolled_back");
        assert!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .is_none(),
            "rolled_back final state should clear active marker"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn execute_with_transaction_preserves_committed_status_on_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut txid = None;
        let err = execute_with_transaction(&layout, "install", None, |tx| {
            txid = Some(tx.txid.clone());
            set_transaction_status(&layout, &tx.txid, "committed")?;
            Err(anyhow::anyhow!("post-commit warning"))
        })
        .expect_err("committed transaction should preserve final status on error");
        assert!(err.to_string().contains("post-commit warning"));

        let txid = txid.expect("txid should be captured");
        let metadata = read_transaction_metadata(&layout, &txid)
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(metadata.status, "committed");
        assert!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .is_none(),
            "committed final state should clear active marker"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_blocks_applying_without_mutating_status() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-applying".to_string(),
            operation: "install".to_string(),
            status: "applying".to_string(),
            started_at_unix: 1_771_001_560,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-applying").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("applying transaction should block concurrent mutation");
        assert!(
            err.to_string().contains(
                "transaction tx-applying is active (reason=active_status status=applying)"
            ),
            "unexpected error: {err}"
        );

        let updated = read_transaction_metadata(&layout, "tx-applying")
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(updated.status, "applying");

        let second_err = ensure_no_active_transaction(&layout)
            .expect_err("second preflight call should remain blocked and deterministic");
        assert!(
            second_err.to_string().contains(
                "transaction tx-applying is active (reason=active_status status=applying)"
            ),
            "unexpected second error: {second_err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_blocks_rolling_back_without_mutating_status() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-rolling-back".to_string(),
            operation: "install".to_string(),
            status: "rolling_back".to_string(),
            started_at_unix: 1_771_001_580,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-rolling-back").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("rolling_back transaction should block and preserve status");
        assert!(
            err.to_string()
                .contains("transaction tx-rolling-back requires repair"),
            "unexpected error: {err}"
        );

        let updated = read_transaction_metadata(&layout, "tx-rolling-back")
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(updated.status, "rolling_back");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_reports_failed_state() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-failed".to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_620,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-failed").expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should resolve for failed tx");
        assert_eq!(line, "transaction: failed tx-failed (reason=failed)");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_treats_rolling_back_as_failed() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-rolling-back".to_string(),
            operation: "uninstall".to_string(),
            status: "rolling_back".to_string(),
            started_at_unix: 1_771_001_630,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-rolling-back").expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should resolve for rolling_back tx");
        assert_eq!(
            line,
            "transaction: failed tx-rolling-back (reason=rolling_back)"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_reports_failed_when_active_marker_unreadable() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        std::fs::create_dir_all(layout.transaction_active_path())
            .expect("must create unreadable active marker fixture");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should map unreadable active marker to failed");
        let expected = format!(
            "transaction: failed (reason=active_marker_unreadable path={})",
            layout.transaction_active_path().display()
        );
        assert_eq!(line, expected);

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_reports_failed_when_active_marker_has_no_metadata() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        set_active_transaction(&layout, "tx-missing").expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should resolve for missing metadata");
        let expected = format!(
            "transaction: failed tx-missing (reason=metadata_missing path={})",
            layout.transaction_metadata_path("tx-missing").display()
        );
        assert_eq!(line, expected);

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_reports_failed_when_metadata_unreadable() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-unreadable";
        std::fs::write(layout.transaction_metadata_path(txid), "{not-json")
            .expect("must write corrupt metadata");
        set_active_transaction(&layout, txid).expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should map unreadable metadata to failed");
        let expected = format!(
            "transaction: failed tx-unreadable (reason=metadata_unreadable path={})",
            layout.transaction_metadata_path(txid).display()
        );
        assert_eq!(line, expected);

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_treats_applying_as_active() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-applying-health".to_string(),
            operation: "install".to_string(),
            status: "applying".to_string(),
            started_at_unix: 1_771_001_645,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-applying-health").expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should resolve for applying tx");
        assert_eq!(line, "transaction: active tx-applying-health");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_reports_active_state_without_status_suffix() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-active".to_string(),
            operation: "upgrade".to_string(),
            status: "paused".to_string(),
            started_at_unix: 1_771_001_640,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-active").expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should resolve for active tx");
        assert_eq!(line, "transaction: active tx-active");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_treats_committed_marker_as_clean() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-committed".to_string(),
            operation: "install".to_string(),
            status: "committed".to_string(),
            started_at_unix: 1_771_001_660,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-committed").expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should resolve for committed marker");
        assert_eq!(line, "transaction: clean");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_treats_planning_marker_as_active() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-planning".to_string(),
            operation: "install".to_string(),
            status: "planning".to_string(),
            started_at_unix: 1_771_001_670,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-planning").expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should resolve for planning marker");
        assert_eq!(line, "transaction: active tx-planning");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_clears_stale_marker_when_status_is_final() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-stale".to_string(),
            operation: "upgrade".to_string(),
            status: "committed".to_string(),
            started_at_unix: 1_771_001_680,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-stale").expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should resolve for stale marker");
        assert_eq!(line, "transaction: clean");
        assert!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .is_none(),
            "doctor should clear stale final-state marker"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn parse_pin_spec_requires_constraint() {
        let err = parse_pin_spec("ripgrep").expect_err("must require constraint");
        assert!(err.to_string().contains("pin requires"));
    }

    #[test]
    fn select_manifest_with_pin_applies_both_constraints() {
        let one = PackageManifest::from_toml_str(
            r#"
name = "tool"
version = "1.2.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/tool-1.2.0.tar.zst"
sha256 = "abc"
"#,
        )
        .expect("manifest must parse");
        let two = PackageManifest::from_toml_str(
            r#"
name = "tool"
version = "1.3.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/tool-1.3.0.tar.zst"
sha256 = "def"
"#,
        )
        .expect("manifest must parse");

        let versions = vec![one, two];
        let request = VersionReq::parse("^1").expect("request req");
        let pin = VersionReq::parse("<1.3.0").expect("pin req");

        let selected =
            select_manifest_with_pin(&versions, &request, Some(&pin)).expect("must select");
        assert_eq!(selected.version.to_string(), "1.2.0");
    }

    #[test]
    fn validate_install_preflight_for_resolved_rejects_unmanaged_bin_in_dry_run() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let existing = bin_path(&layout, "rg");
        fs::write(&existing, b"#!/bin/sh\n").expect("must write existing file");

        let manifest = PackageManifest::from_toml_str(
            r#"
name = "ripgrep"
version = "15.1.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/ripgrep-15.1.0.tar.gz"
sha256 = "abc"
[[artifacts.binaries]]
name = "rg"
path = "rg"
"#,
        )
        .expect("manifest should parse");
        let resolved = ResolvedInstall {
            artifact: manifest.artifacts[0].clone(),
            manifest,
            resolved_target: "x86_64-unknown-linux-gnu".to_string(),
            archive_type: ArchiveType::TarGz,
        };

        let err = validate_install_preflight_for_resolved(&layout, &resolved, &[])
            .expect_err("dry-run preflight should reject unmanaged binary conflicts");
        assert!(err
            .to_string()
            .contains("already exists and is not managed by crosspack"));

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn validate_binary_preflight_rejects_other_package_owner() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let receipts = vec![InstallReceipt {
            name: "fd".to_string(),
            version: "10.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let err = validate_binary_preflight(
            &layout,
            "ripgrep",
            &["rg".to_string()],
            &receipts,
            &HashSet::new(),
        )
        .expect_err("must reject conflict");
        assert!(err.to_string().contains("already owned by package 'fd'"));

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn validate_binary_preflight_rejects_unmanaged_existing_file() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let existing = bin_path(&layout, "rg");
        fs::write(&existing, b"#!/bin/sh\n").expect("must write existing file");

        let err = validate_binary_preflight(
            &layout,
            "ripgrep",
            &["rg".to_string()],
            &[],
            &HashSet::new(),
        )
        .expect_err("must reject unmanaged file");
        assert!(err
            .to_string()
            .contains("already exists and is not managed by crosspack"));

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn validate_binary_preflight_allows_replacement_owned_binary() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let existing = bin_path(&layout, "rg");
        fs::write(&existing, b"#!/bin/sh\n").expect("must write existing file");

        let receipts = vec![InstallReceipt {
            name: "ripgrep-legacy".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let replacement_targets = HashSet::from(["ripgrep-legacy"]);
        validate_binary_preflight(
            &layout,
            "ripgrep",
            &["rg".to_string()],
            &receipts,
            &replacement_targets,
        )
        .expect("replacement-owned binary should be allowed");

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn validate_binary_preflight_allows_self_update_current_exe_binary() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let existing = bin_path(&layout, "crosspack");
        fs::write(&existing, b"#!/bin/sh\n").expect("must write existing file");

        validate_binary_preflight_with_current_exe(
            &layout,
            "crosspack",
            &["crosspack".to_string()],
            &[],
            &HashSet::new(),
            Some(existing.as_path()),
        )
        .expect("self-update should allow replacing the currently running crosspack binary");

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn validate_completion_preflight_rejects_other_package_owner() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let desired = "packages/bash/zoxide--completions--zoxide.bash".to_string();
        let receipts = vec![InstallReceipt {
            name: "zoxide".to_string(),
            version: "0.9.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["zoxide".to_string()],
            exposed_completions: vec![desired.clone()],
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let err = validate_completion_preflight(
            &layout,
            "ripgrep",
            std::slice::from_ref(&desired),
            &receipts,
        )
        .expect_err("must reject completion ownership conflict");
        assert!(err
            .to_string()
            .contains("is already owned by package 'zoxide'"));

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn validate_completion_preflight_rejects_unmanaged_existing_file() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let desired = "packages/bash/ripgrep--completions--rg.bash".to_string();
        let path =
            exposed_completion_path(&layout, &desired).expect("must resolve completion path");
        fs::create_dir_all(path.parent().expect("must have parent"))
            .expect("must create completion parent");
        fs::write(&path, b"complete -F _rg rg\n").expect("must write completion file");

        let err =
            validate_completion_preflight(&layout, "ripgrep", std::slice::from_ref(&desired), &[])
                .expect_err("must reject unmanaged completion file");
        assert!(err
            .to_string()
            .contains("already exists and is not managed by crosspack"));

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn validate_completion_preflight_allows_self_owned_existing_file() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let desired = "packages/bash/ripgrep--completions--rg.bash".to_string();
        let path =
            exposed_completion_path(&layout, &desired).expect("must resolve completion path");
        fs::create_dir_all(path.parent().expect("must have parent"))
            .expect("must create completion parent");
        fs::write(&path, b"complete -F _rg rg\n").expect("must write completion file");

        let receipts = vec![InstallReceipt {
            name: "ripgrep".to_string(),
            version: "14.1.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: vec![desired.clone()],
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        validate_completion_preflight(
            &layout,
            "ripgrep",
            std::slice::from_ref(&desired),
            &receipts,
        )
        .expect("self-owned completion file should be allowed");

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn validate_gui_preflight_rejects_other_package_owner() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        write_gui_exposure_state(
            &layout,
            "zed",
            &[GuiExposureAsset {
                key: "protocol:zed".to_string(),
                rel_path: "handlers/zed--app.meta".to_string(),
            }],
        )
        .expect("must seed gui ownership");

        let desired = vec![GuiExposureAsset {
            key: "protocol:zed".to_string(),
            rel_path: "handlers/newapp.meta".to_string(),
        }];

        let err = validate_gui_preflight(&layout, "other", &desired, &HashSet::new())
            .expect_err("must reject gui ownership conflict");
        assert!(err.to_string().contains("already owned by package 'zed'"));

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn validate_gui_preflight_rejects_unmanaged_existing_file() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let desired = vec![GuiExposureAsset {
            key: "app:dev.demo.app".to_string(),
            rel_path: "launchers/demo--app.command".to_string(),
        }];

        let unmanaged_path =
            gui_asset_path(&layout, &desired[0].rel_path).expect("must resolve gui path");
        fs::create_dir_all(unmanaged_path.parent().expect("must have parent"))
            .expect("must create parent");
        fs::write(&unmanaged_path, b"#!/bin/sh\n").expect("must write unmanaged gui file");

        let err = validate_gui_preflight(&layout, "demo", &desired, &HashSet::new())
            .expect_err("must reject unmanaged existing gui file");
        assert!(err
            .to_string()
            .contains("already exists and is not managed by crosspack"));

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn validate_gui_preflight_allows_self_owned_existing_file() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let desired = vec![GuiExposureAsset {
            key: "app:dev.demo.app".to_string(),
            rel_path: "launchers/demo--app.command".to_string(),
        }];

        let managed_path =
            gui_asset_path(&layout, &desired[0].rel_path).expect("must resolve gui path");
        fs::create_dir_all(managed_path.parent().expect("must have parent"))
            .expect("must create parent");
        fs::write(&managed_path, b"#!/bin/sh\n").expect("must write managed gui file");
        write_gui_exposure_state(&layout, "demo", &desired).expect("must seed self-owned gui file");

        validate_gui_preflight(&layout, "demo", &desired, &HashSet::new())
            .expect("self-owned gui file should be allowed");

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn collect_declared_gui_assets_rejects_colliding_projected_paths() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "demo"
version = "1.0.0"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/demo.tar.gz"
sha256 = "abc123"

[[artifacts.gui_apps]]
app_id = "dev.demo/App"
display_name = "Demo Slash"
exec = "demo"

[[artifacts.gui_apps]]
app_id = "dev.demo?App"
display_name = "Demo Question"
exec = "demo"
"#,
        )
        .expect("manifest should parse");
        let artifact = manifest
            .artifacts
            .first()
            .expect("manifest should include one artifact");

        let err = collect_declared_gui_assets(&manifest.name, artifact)
            .expect_err("colliding projected gui paths must be rejected");
        assert!(
            err.to_string()
                .contains("duplicate gui storage path declaration"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn collect_declared_gui_assets_allows_shared_handler_path_within_single_app() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "demo"
version = "1.0.0"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/demo.tar.gz"
sha256 = "abc123"

[[artifacts.gui_apps]]
app_id = "dev.demo.App"
display_name = "Demo"
exec = "demo"

[[artifacts.gui_apps.protocols]]
scheme = "demo"
"#,
        )
        .expect("manifest should parse");
        let artifact = manifest
            .artifacts
            .first()
            .expect("manifest should include one artifact");

        let assets = collect_declared_gui_assets(&manifest.name, artifact)
            .expect("single app should allow shared handler paths");
        assert!(!assets.is_empty());
    }

    #[test]
    fn collect_replacement_receipts_matches_manifest_rules() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "ripgrep"
version = "2.0.0"

[replaces]
ripgrep-legacy = "<2.0.0"
"#,
        )
        .expect("manifest should parse");

        let receipts = vec![
            InstallReceipt {
                name: "ripgrep-legacy".to_string(),
                version: "1.5.0".to_string(),
                dependencies: Vec::new(),
                target: None,
                artifact_url: None,
                artifact_sha256: None,
                cache_path: None,
                exposed_bins: vec!["rg".to_string()],
                exposed_completions: Vec::new(),
                snapshot_id: None,
                install_mode: InstallMode::Managed,
                install_reason: InstallReason::Root,
                install_status: "installed".to_string(),
                installed_at_unix: 1,
            },
            InstallReceipt {
                name: "other".to_string(),
                version: "3.0.0".to_string(),
                dependencies: Vec::new(),
                target: None,
                artifact_url: None,
                artifact_sha256: None,
                cache_path: None,
                exposed_bins: vec!["other".to_string()],
                exposed_completions: Vec::new(),
                snapshot_id: None,
                install_mode: InstallMode::Managed,
                install_reason: InstallReason::Dependency,
                install_status: "installed".to_string(),
                installed_at_unix: 1,
            },
        ];

        let replacements =
            collect_replacement_receipts(&manifest, &receipts).expect("replacement match expected");
        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].name, "ripgrep-legacy");
    }

    #[test]
    fn collect_replacement_receipts_rejects_invalid_installed_version() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "ripgrep"
version = "2.0.0"

[replaces]
ripgrep-legacy = "*"
"#,
        )
        .expect("manifest should parse");

        let receipts = vec![InstallReceipt {
            name: "ripgrep-legacy".to_string(),
            version: "not-a-semver".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let err = collect_replacement_receipts(&manifest, &receipts)
            .expect_err("invalid installed semver should fail replacement preflight");
        assert!(err
            .to_string()
            .contains("invalid version for replacement preflight"));
    }

    #[test]
    fn apply_replacement_handoff_blocks_when_dependents_remain() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let app = InstallReceipt {
            name: "app".to_string(),
            version: "1.0.0".to_string(),
            dependencies: vec!["ripgrep-legacy@1.0.0".to_string()],
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["app".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        let replaced = InstallReceipt {
            name: "ripgrep-legacy".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        write_install_receipt(&layout, &app).expect("must seed app receipt");
        write_install_receipt(&layout, &replaced).expect("must seed replaced receipt");

        let err =
            apply_replacement_handoff(&layout, std::slice::from_ref(&replaced), &HashMap::new())
                .expect_err("replacement must fail while rooted dependents remain");
        assert!(err.to_string().contains("still required by roots app"));

        let remaining = read_install_receipts(&layout).expect("must read receipts");
        assert_eq!(
            remaining.len(),
            2,
            "blocked replacement must not mutate state"
        );

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn apply_replacement_handoff_preflights_all_targets_before_mutation() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let app = InstallReceipt {
            name: "app".to_string(),
            version: "1.0.0".to_string(),
            dependencies: vec!["legacy-b@1.0.0".to_string()],
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["app".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        let legacy_a = InstallReceipt {
            name: "legacy-a".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["legacy-a".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Dependency,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        let legacy_b = InstallReceipt {
            name: "legacy-b".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["legacy-b".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Dependency,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        write_install_receipt(&layout, &app).expect("must seed app receipt");
        write_install_receipt(&layout, &legacy_a).expect("must seed first replacement target");
        write_install_receipt(&layout, &legacy_b).expect("must seed second replacement target");

        let err = apply_replacement_handoff(
            &layout,
            &[legacy_a.clone(), legacy_b.clone()],
            &HashMap::new(),
        )
        .expect_err("blocked replacement must fail before any uninstall mutation");
        assert!(err.to_string().contains("still required by roots app"));

        let remaining = read_install_receipts(&layout).expect("must read receipts");
        let remaining_names = remaining
            .iter()
            .map(|receipt| receipt.name.as_str())
            .collect::<HashSet<_>>();
        assert!(
            remaining_names.contains("legacy-a") && remaining_names.contains("legacy-b"),
            "preflight failure must keep every replacement target installed"
        );

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn apply_replacement_handoff_allows_interdependent_replacement_roots() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let legacy_a = InstallReceipt {
            name: "legacy-a".to_string(),
            version: "1.0.0".to_string(),
            dependencies: vec!["legacy-b@1.0.0".to_string()],
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["legacy-a".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        let legacy_b = InstallReceipt {
            name: "legacy-b".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["legacy-b".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        write_install_receipt(&layout, &legacy_a).expect("must seed first replacement root");
        write_install_receipt(&layout, &legacy_b).expect("must seed second replacement root");

        apply_replacement_handoff(
            &layout,
            &[legacy_a.clone(), legacy_b.clone()],
            &HashMap::new(),
        )
        .expect("replacement handoff should allow roots that are all being replaced");

        let remaining = read_install_receipts(&layout).expect("must read receipts");
        assert!(
            remaining.is_empty(),
            "all replacement roots should be removed in a successful handoff"
        );

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn apply_replacement_handoff_uses_planned_dependency_overrides() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let app = InstallReceipt {
            name: "app".to_string(),
            version: "1.0.0".to_string(),
            dependencies: vec!["ripgrep-legacy@1.0.0".to_string()],
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["app".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        let replaced = InstallReceipt {
            name: "ripgrep-legacy".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Dependency,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        write_install_receipt(&layout, &app).expect("must seed app receipt");
        write_install_receipt(&layout, &replaced).expect("must seed replaced receipt");

        let planned_dependency_overrides =
            HashMap::from([("app".to_string(), vec!["ripgrep".to_string()])]);

        apply_replacement_handoff(
            &layout,
            std::slice::from_ref(&replaced),
            &planned_dependency_overrides,
        )
        .expect("planned dependency graph should allow replacement handoff");

        let remaining = read_install_receipts(&layout).expect("must read receipts");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].name, "app");

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn apply_replacement_handoff_uninstalls_safe_target() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let replaced = InstallReceipt {
            name: "ripgrep-legacy".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        write_install_receipt(&layout, &replaced).expect("must seed replaced receipt");

        apply_replacement_handoff(&layout, std::slice::from_ref(&replaced), &HashMap::new())
            .expect("safe replacement handoff should uninstall target");

        let remaining = read_install_receipts(&layout).expect("must read receipts");
        assert!(
            remaining.is_empty(),
            "replacement handoff must remove target receipt"
        );

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn enforce_no_downgrades_rejects_lower_version() {
        let receipts = vec![InstallReceipt {
            name: "tool".to_string(),
            version: "2.0.0".to_string(),
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
        }];
        let resolved = vec![resolved_install("tool", "1.9.0")];

        let err = enforce_no_downgrades(&receipts, &resolved, "upgrade").expect_err("must fail");
        assert!(err.to_string().contains("would downgrade 'tool'"));
    }

    #[test]
    fn enforce_no_downgrades_allows_upgrade() {
        let receipts = vec![InstallReceipt {
            name: "tool".to_string(),
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
        }];
        let resolved = vec![resolved_install("tool", "1.2.0")];
        enforce_no_downgrades(&receipts, &resolved, "upgrade").expect("must pass");
    }

    #[test]
    fn determine_install_reason_sets_requested_root() {
        let reason = determine_install_reason("tool", &["tool".to_string()], &[], &[]);
        assert_eq!(reason, InstallReason::Root);
    }

    #[test]
    fn determine_install_reason_sets_dependency_for_non_root() {
        let reason = determine_install_reason("shared", &["app".to_string()], &[], &[]);
        assert_eq!(reason, InstallReason::Dependency);
    }

    #[test]
    fn determine_install_reason_preserves_existing_root() {
        let existing = vec![InstallReceipt {
            name: "shared".to_string(),
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
        }];

        let reason = determine_install_reason("shared", &["app".to_string()], &existing, &[]);
        assert_eq!(reason, InstallReason::Root);
    }

    #[test]
    fn determine_install_reason_promotes_to_root_when_requested() {
        let existing = vec![InstallReceipt {
            name: "shared".to_string(),
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
            install_reason: InstallReason::Dependency,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let reason = determine_install_reason("shared", &["shared".to_string()], &existing, &[]);
        assert_eq!(reason, InstallReason::Root);
    }

    #[test]
    fn determine_install_reason_promotes_existing_dependency_when_replacing_root() {
        let existing = vec![InstallReceipt {
            name: "ripgrep".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Dependency,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];
        let replacement = vec![InstallReceipt {
            name: "ripgrep-legacy".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let reason = determine_install_reason("ripgrep", &[], &existing, &replacement);
        assert_eq!(reason, InstallReason::Root);
    }

    #[test]
    fn determine_install_reason_preserves_root_from_replacement_target() {
        let replacement = vec![InstallReceipt {
            name: "ripgrep-legacy".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let reason = determine_install_reason("ripgrep", &[], &[], &replacement);
        assert_eq!(reason, InstallReason::Root);
    }

    #[test]
    fn build_upgrade_roots_uses_only_root_receipts() {
        let receipts = vec![
            InstallReceipt {
                name: "app".to_string(),
                version: "1.0.0".to_string(),
                dependencies: vec!["shared@1.0.0".to_string()],
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
            InstallReceipt {
                name: "shared".to_string(),
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
                install_reason: InstallReason::Dependency,
                install_status: "installed".to_string(),
                installed_at_unix: 1,
            },
        ];

        let roots = build_upgrade_roots(&receipts);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].name, "app");
    }

    #[test]
    fn build_upgrade_roots_is_empty_when_no_roots_installed() {
        let receipts = vec![InstallReceipt {
            name: "shared".to_string(),
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
            install_reason: InstallReason::Dependency,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let roots = build_upgrade_roots(&receipts);
        assert!(roots.is_empty());
    }

    #[test]
    fn build_upgrade_plans_groups_roots_by_target() {
        let receipts = vec![
            InstallReceipt {
                name: "linux-tool".to_string(),
                version: "1.0.0".to_string(),
                dependencies: Vec::new(),
                target: Some("x86_64-unknown-linux-gnu".to_string()),
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
            InstallReceipt {
                name: "mac-tool".to_string(),
                version: "1.0.0".to_string(),
                dependencies: Vec::new(),
                target: Some("aarch64-apple-darwin".to_string()),
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
        ];

        let plans = build_upgrade_plans(&receipts);
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].target.as_deref(), Some("aarch64-apple-darwin"));
        assert_eq!(plans[0].root_names, vec!["mac-tool"]);
        assert_eq!(plans[1].target.as_deref(), Some("x86_64-unknown-linux-gnu"));
        assert_eq!(plans[1].root_names, vec!["linux-tool"]);
    }

    #[test]
    fn build_upgrade_plans_ignores_dependency_receipts() {
        let receipts = vec![
            InstallReceipt {
                name: "app".to_string(),
                version: "1.0.0".to_string(),
                dependencies: vec!["shared@1.0.0".to_string()],
                target: Some("x86_64-unknown-linux-gnu".to_string()),
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
            InstallReceipt {
                name: "shared".to_string(),
                version: "1.0.0".to_string(),
                dependencies: Vec::new(),
                target: Some("x86_64-unknown-linux-gnu".to_string()),
                artifact_url: None,
                artifact_sha256: None,
                cache_path: None,
                exposed_bins: Vec::new(),
                exposed_completions: Vec::new(),
                snapshot_id: None,
                install_mode: InstallMode::Managed,
                install_reason: InstallReason::Dependency,
                install_status: "installed".to_string(),
                installed_at_unix: 1,
            },
        ];

        let plans = build_upgrade_plans(&receipts);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].root_names, vec!["app"]);
        assert_eq!(plans[0].roots.len(), 1);
        assert_eq!(plans[0].roots[0].name, "app");
    }

    #[test]
    fn build_upgrade_plans_is_empty_when_no_roots_installed() {
        let receipts = vec![InstallReceipt {
            name: "shared".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: Vec::new(),
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Dependency,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let plans = build_upgrade_plans(&receipts);
        assert!(plans.is_empty());
    }

    #[test]
    fn enforce_disjoint_multi_target_upgrade_rejects_overlapping_package_names() {
        let err = enforce_disjoint_multi_target_upgrade(&[
            (
                Some("x86_64-unknown-linux-gnu"),
                vec!["shared".to_string(), "linux-tool".to_string()],
            ),
            (
                Some("aarch64-apple-darwin"),
                vec!["shared".to_string(), "mac-tool".to_string()],
            ),
        ])
        .expect_err("overlap must fail");

        assert!(err
            .to_string()
            .contains("cannot safely process package 'shared'"));
        assert!(err.to_string().contains("separate prefixes"));
    }

    #[test]
    fn enforce_disjoint_multi_target_upgrade_allows_disjoint_package_sets() {
        enforce_disjoint_multi_target_upgrade(&[
            (
                Some("x86_64-unknown-linux-gnu"),
                vec!["linux-tool".to_string(), "linux-lib".to_string()],
            ),
            (
                Some("aarch64-apple-darwin"),
                vec!["mac-tool".to_string(), "mac-lib".to_string()],
            ),
        ])
        .expect("disjoint groups must pass");
    }

    #[test]
    fn format_uninstall_messages_reports_blocking_roots() {
        let result = UninstallResult {
            name: "shared".to_string(),
            version: Some("1.0.0".to_string()),
            status: UninstallStatus::BlockedByDependents,
            pruned_dependencies: Vec::new(),
            blocked_by_roots: vec!["app-a".to_string(), "app-b".to_string()],
        };

        let lines = format_uninstall_messages(&result);
        assert_eq!(
            lines,
            vec!["cannot uninstall shared 1.0.0: still required by roots app-a, app-b".to_string()]
        );
    }

    #[test]
    fn format_uninstall_messages_reports_pruned_dependencies() {
        let result = UninstallResult {
            name: "app".to_string(),
            version: Some("1.0.0".to_string()),
            status: UninstallStatus::Uninstalled,
            pruned_dependencies: vec!["shared".to_string(), "zlib".to_string()],
            blocked_by_roots: Vec::new(),
        };

        let lines = format_uninstall_messages(&result);
        assert_eq!(lines[0], "uninstalled app 1.0.0");
        assert_eq!(lines[1], "pruned orphan dependencies: shared, zlib");
    }

    #[test]
    fn install_defaults_to_auto_escalation_when_interactive() {
        let cli =
            Cli::try_parse_from(["crosspack", "install", "ripgrep"]).expect("command must parse");

        match cli.command {
            Commands::Install { escalation, .. } => {
                let policy = resolve_escalation_policy(escalation);
                assert!(policy.allow_prompt_escalation);
                assert!(policy.allow_non_prompt_escalation);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn non_interactive_disables_prompt_escalation() {
        let policy = resolve_escalation_policy(EscalationArgs {
            non_interactive: true,
            allow_escalation: false,
            no_escalation: false,
        });

        assert!(!policy.allow_prompt_escalation);
        assert!(!policy.allow_non_prompt_escalation);
    }

    #[test]
    fn non_interactive_allow_escalation_enables_non_prompt_paths() {
        let policy = resolve_escalation_policy(EscalationArgs {
            non_interactive: true,
            allow_escalation: true,
            no_escalation: false,
        });

        assert!(!policy.allow_prompt_escalation);
        assert!(policy.allow_non_prompt_escalation);
    }

    #[test]
    fn no_escalation_overrides_interactive_default() {
        let policy = resolve_escalation_policy(EscalationArgs {
            non_interactive: false,
            allow_escalation: false,
            no_escalation: true,
        });

        assert!(!policy.allow_prompt_escalation);
        assert!(!policy.allow_non_prompt_escalation);
    }

    #[test]
    fn install_mode_for_archive_type_defaults_native_for_installer_artifacts() {
        for archive_type in [
            ArchiveType::Msi,
            ArchiveType::Exe,
            ArchiveType::Pkg,
            ArchiveType::Msix,
            ArchiveType::Appx,
        ] {
            assert_eq!(
                install_mode_for_archive_type(archive_type),
                InstallMode::Native
            );
        }
    }

    #[test]
    fn install_mode_for_archive_type_defaults_managed_for_archive_payloads() {
        for archive_type in [
            ArchiveType::Zip,
            ArchiveType::TarGz,
            ArchiveType::TarZst,
            ArchiveType::Bin,
            ArchiveType::Dmg,
            ArchiveType::AppImage,
        ] {
            assert_eq!(
                install_mode_for_archive_type(archive_type),
                InstallMode::Managed
            );
        }
    }

    #[test]
    fn install_interaction_policy_matches_escalation_policy_flags() {
        let interaction_policy = install_interaction_policy(EscalationPolicy {
            allow_prompt_escalation: false,
            allow_non_prompt_escalation: true,
        });

        assert!(!interaction_policy.allow_prompt_escalation);
        assert!(interaction_policy.allow_non_prompt_escalation);
    }

    #[test]
    fn build_artifact_install_options_carries_mode_and_interaction_policy() {
        let mut resolved = resolved_install("demo", "1.0.0");
        resolved.archive_type = ArchiveType::Exe;
        resolved.artifact.strip_components = Some(2);
        resolved.artifact.artifact_root = Some("payload".to_string());

        let interaction_policy = install_interaction_policy(EscalationPolicy {
            allow_prompt_escalation: false,
            allow_non_prompt_escalation: true,
        });
        let options = build_artifact_install_options(&resolved, interaction_policy);

        assert_eq!(options.strip_components, 2);
        assert_eq!(options.artifact_root, Some("payload"));
        assert_eq!(options.install_mode, InstallMode::Native);
        assert_eq!(options.interaction_policy, interaction_policy);
    }

    #[test]
    fn bin_cache_file_name_from_url_uses_final_path_segment() {
        let file_name = bin_cache_file_name_from_url(
            "https://example.test/releases/download/v1.0.0/tool-macos-arm64?download=1#asset",
        )
        .expect("must derive file name");
        assert_eq!(file_name, "tool-macos-arm64");
    }

    #[test]
    fn resolved_artifact_cache_path_uses_url_file_name_for_bin_artifacts() {
        let layout = test_layout();
        let cache_path = resolved_artifact_cache_path(
            &layout,
            "jq",
            "1.8.1",
            "aarch64-apple-darwin",
            ArchiveType::Bin,
            "https://example.test/releases/download/jq-1.8.1/jq-macos-arm64",
        )
        .expect("must resolve cache path");

        assert_eq!(
            cache_path,
            layout
                .prefix()
                .join("cache/artifacts/jq/1.8.1/aarch64-apple-darwin/jq-macos-arm64")
        );
    }

    #[test]
    fn cli_parses_install_with_repeatable_provider_overrides() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "install",
            "compiler@^2",
            "--provider",
            "c-compiler=clang",
            "--provider",
            "rust-toolchain=rustup",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::Install {
                dry_run,
                provider,
                escalation,
                ..
            } => {
                assert!(!dry_run);
                assert_eq!(provider, vec!["c-compiler=clang", "rust-toolchain=rustup"]);
                assert!(!escalation.non_interactive);
                assert!(!escalation.allow_escalation);
                assert!(!escalation.no_escalation);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_install_with_dry_run_flag() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "install",
            "ripgrep",
            "--dry-run",
            "--non-interactive",
            "--allow-escalation",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::Install {
                dry_run,
                escalation,
                ..
            } => {
                assert!(dry_run);
                assert!(escalation.non_interactive);
                assert!(escalation.allow_escalation);
                assert!(!escalation.no_escalation);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_rejects_install_with_conflicting_escalation_flags() {
        let err = Cli::try_parse_from([
            "crosspack",
            "install",
            "ripgrep",
            "--allow-escalation",
            "--no-escalation",
        ])
        .expect_err("conflicting escalation flags must fail");

        assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn cli_parses_upgrade_with_repeatable_provider_overrides() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "upgrade",
            "compiler@^2",
            "--provider",
            "c-compiler=clang",
            "--provider",
            "rust-toolchain=rustup",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::Upgrade {
                dry_run,
                provider,
                escalation,
                ..
            } => {
                assert!(!dry_run);
                assert_eq!(provider, vec!["c-compiler=clang", "rust-toolchain=rustup"]);
                assert!(!escalation.non_interactive);
                assert!(!escalation.allow_escalation);
                assert!(!escalation.no_escalation);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_upgrade_with_dry_run_flag() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "upgrade",
            "ripgrep",
            "--dry-run",
            "--non-interactive",
            "--allow-escalation",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::Upgrade {
                dry_run,
                escalation,
                ..
            } => {
                assert!(dry_run);
                assert!(escalation.non_interactive);
                assert!(escalation.allow_escalation);
                assert!(!escalation.no_escalation);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_uninstall_with_escalation_flags() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "uninstall",
            "ripgrep",
            "--non-interactive",
            "--allow-escalation",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::Uninstall { name, escalation } => {
                assert_eq!(name, "ripgrep");
                assert!(escalation.non_interactive);
                assert!(escalation.allow_escalation);
                assert!(!escalation.no_escalation);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_rollback_with_escalation_flags() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "rollback",
            "tx-123",
            "--non-interactive",
            "--allow-escalation",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::Rollback { txid, escalation } => {
                assert_eq!(txid.as_deref(), Some("tx-123"));
                assert!(escalation.non_interactive);
                assert!(escalation.allow_escalation);
                assert!(!escalation.no_escalation);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_repair_with_escalation_flags() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "repair",
            "--non-interactive",
            "--allow-escalation",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::Repair { escalation } => {
                assert!(escalation.non_interactive);
                assert!(escalation.allow_escalation);
                assert!(!escalation.no_escalation);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_completions_for_each_supported_shell() {
        let cases = vec![
            ("bash", CliCompletionShell::Bash),
            ("zsh", CliCompletionShell::Zsh),
            ("fish", CliCompletionShell::Fish),
            ("powershell", CliCompletionShell::Powershell),
        ];

        for (shell, expected) in cases {
            let cli =
                Cli::try_parse_from(["crosspack", "completions", shell]).expect("command parses");
            match cli.command {
                Commands::Completions { shell } => {
                    assert_eq!(shell, expected);
                }
                other => panic!("unexpected command: {other:?}"),
            }
        }
    }

    #[test]
    fn cli_rejects_completions_without_shell() {
        let err = Cli::try_parse_from(["crosspack", "completions"])
            .expect_err("missing shell argument must fail");
        assert!(err.to_string().contains("<SHELL>"));
    }

    #[test]
    fn cli_rejects_unsupported_completion_shell() {
        let err = Cli::try_parse_from(["crosspack", "completions", "elvish"])
            .expect_err("unsupported shell must fail");
        let rendered = err.to_string();
        assert!(rendered.contains("elvish"));
        assert!(rendered.contains("possible values"));
    }

    #[test]
    fn cli_parses_init_shell_with_optional_shell_override() {
        let cli = Cli::try_parse_from(["crosspack", "init-shell", "--shell", "zsh"])
            .expect("command must parse");
        match cli.command {
            Commands::InitShell { shell } => {
                assert_eq!(shell, Some(CliCompletionShell::Zsh));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_self_update_with_optional_flags() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "self-update",
            "--dry-run",
            "--force-redownload",
            "--non-interactive",
            "--allow-escalation",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::SelfUpdate {
                dry_run,
                force_redownload,
                escalation,
            } => {
                assert!(dry_run);
                assert!(force_redownload);
                assert!(escalation.non_interactive);
                assert!(escalation.allow_escalation);
                assert!(!escalation.no_escalation);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_version_subcommand() {
        let cli = Cli::try_parse_from(["crosspack", "version"]).expect("command must parse");
        assert!(matches!(cli.command, Commands::Version));
    }

    #[test]
    fn build_self_update_install_args_includes_registry_root_and_flags() {
        let registry_root = PathBuf::from("/tmp/registry");
        let args = build_self_update_install_args(
            Some(registry_root.as_path()),
            true,
            true,
            EscalationArgs {
                non_interactive: true,
                allow_escalation: true,
                no_escalation: false,
            },
        );
        let rendered = args
            .iter()
            .map(|value| value.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            rendered,
            vec![
                "--registry-root",
                "/tmp/registry",
                "install",
                "crosspack",
                "--dry-run",
                "--force-redownload",
                "--non-interactive",
                "--allow-escalation",
            ]
        );
    }

    #[test]
    fn build_self_update_install_args_omits_optional_values() {
        let args = build_self_update_install_args(None, false, false, EscalationArgs::default());
        let rendered = args
            .iter()
            .map(|value| value.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(rendered, vec!["install", "crosspack"]);
    }

    #[test]
    fn cli_supports_global_version_flag() {
        let err = Cli::try_parse_from(["crosspack", "--version"])
            .expect_err("version flag should exit with version output");
        assert_eq!(err.kind(), ErrorKind::DisplayVersion);
    }

    #[test]
    fn render_transaction_preview_lines_is_deterministic_and_script_friendly() {
        let preview = build_transaction_preview(
            "upgrade",
            &[
                PlannedPackageChange {
                    name: "tool".to_string(),
                    target: "x86_64-unknown-linux-gnu".to_string(),
                    new_version: "2.0.0".to_string(),
                    old_version: Some("1.0.0".to_string()),
                    replacement_removals: vec![PlannedRemoval {
                        name: "old-tool".to_string(),
                        version: "0.9.0".to_string(),
                    }],
                },
                PlannedPackageChange {
                    name: "dep".to_string(),
                    target: "x86_64-unknown-linux-gnu".to_string(),
                    new_version: "1.1.0".to_string(),
                    old_version: None,
                    replacement_removals: Vec::new(),
                },
            ],
        );

        let lines = render_transaction_preview_lines(&preview, TransactionPreviewMode::DryRun);
        assert_eq!(
            lines[0],
            "transaction_preview operation=upgrade mode=dry-run"
        );
        assert_eq!(
            lines[1],
            "transaction_summary adds=1 removals=1 replacements=1 transitions=1"
        );
        assert_eq!(
            lines[2],
            "risk_flags=adds,multi-package-transaction,removals,replacements,version-transitions"
        );
        assert_eq!(
            lines[3],
            "change_add name=dep version=1.1.0 target=x86_64-unknown-linux-gnu"
        );
        assert_eq!(
            lines[4],
            "change_remove name=old-tool version=0.9.0 reason=replacement"
        );
        assert_eq!(lines[5], "change_replace from=old-tool@0.9.0 to=tool@2.0.0");
        assert_eq!(lines[6], "change_transition name=tool from=1.0.0 to=2.0.0");
    }

    #[test]
    fn transaction_preview_dry_run_output_is_stable_for_same_plan() {
        let preview = build_transaction_preview(
            "install",
            &[PlannedPackageChange {
                name: "tool".to_string(),
                target: "x86_64-unknown-linux-gnu".to_string(),
                new_version: "1.2.3".to_string(),
                old_version: Some("1.2.2".to_string()),
                replacement_removals: Vec::new(),
            }],
        );
        let first = render_transaction_preview_lines(&preview, TransactionPreviewMode::DryRun);
        let second = render_transaction_preview_lines(&preview, TransactionPreviewMode::DryRun);

        assert_eq!(first, second);
        assert_eq!(
            first[0],
            "transaction_preview operation=install mode=dry-run"
        );
    }

    #[test]
    fn transaction_preview_omits_multi_package_flag_when_no_mutations() {
        let preview = build_transaction_preview(
            "upgrade",
            &[
                PlannedPackageChange {
                    name: "a".to_string(),
                    target: "x86_64-unknown-linux-gnu".to_string(),
                    new_version: "1.0.0".to_string(),
                    old_version: Some("1.0.0".to_string()),
                    replacement_removals: Vec::new(),
                },
                PlannedPackageChange {
                    name: "b".to_string(),
                    target: "x86_64-unknown-linux-gnu".to_string(),
                    new_version: "2.0.0".to_string(),
                    old_version: Some("2.0.0".to_string()),
                    replacement_removals: Vec::new(),
                },
            ],
        );

        let lines = render_transaction_preview_lines(&preview, TransactionPreviewMode::DryRun);
        assert_eq!(
            lines[1],
            "transaction_summary adds=0 removals=0 replacements=0 transitions=0"
        );
        assert_eq!(lines[2], "risk_flags=none");
    }

    #[test]
    fn resolve_init_shell_prefers_requested_shell_over_env_detection() {
        let resolved = resolve_init_shell(Some(CliCompletionShell::Fish), Some("/bin/zsh"), false);
        assert_eq!(resolved, CliCompletionShell::Fish);
    }

    #[test]
    fn resolve_init_shell_uses_env_detection_when_request_missing() {
        let resolved = resolve_init_shell(None, Some("/usr/bin/pwsh"), false);
        assert_eq!(resolved, CliCompletionShell::Powershell);
    }

    #[test]
    fn resolve_init_shell_falls_back_deterministically_by_platform() {
        let unix_fallback = resolve_init_shell(None, Some("/usr/bin/unknown-shell"), false);
        assert_eq!(unix_fallback, CliCompletionShell::Bash);

        let windows_fallback = resolve_init_shell(None, None, true);
        assert_eq!(windows_fallback, CliCompletionShell::Powershell);
    }

    #[test]
    fn generate_completions_outputs_non_empty_script_for_each_shell() {
        let shells = [
            CliCompletionShell::Bash,
            CliCompletionShell::Zsh,
            CliCompletionShell::Fish,
            CliCompletionShell::Powershell,
        ];
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        for shell in shells {
            let mut output = Vec::new();
            write_completions_script(shell, &layout, &mut output)
                .expect("completion script generation should succeed");
            assert!(
                !output.is_empty(),
                "completion script should not be empty for {shell:?}"
            );
        }
        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn generate_completions_uses_crosspack_command_name() {
        let shells = [
            CliCompletionShell::Bash,
            CliCompletionShell::Zsh,
            CliCompletionShell::Fish,
            CliCompletionShell::Powershell,
        ];
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        for shell in shells {
            let mut output = Vec::new();
            write_completions_script(shell, &layout, &mut output)
                .expect("completion script generation should succeed");
            let rendered = String::from_utf8(output).expect("completion script should be utf-8");
            assert!(
                rendered.contains("crosspack"),
                "completion script should target canonical binary name for {shell:?}"
            );
        }
        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn zsh_completion_script_uses_fpath_for_package_completions() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut output = Vec::new();
        write_completions_script(CliCompletionShell::Zsh, &layout, &mut output)
            .expect("completion script generation should succeed");
        let rendered = String::from_utf8(output).expect("completion script should be utf-8");

        assert!(
            rendered.contains("fpath=('"),
            "zsh loader should register package completion directory via fpath"
        );
        assert!(
            rendered.contains("compinit -i"),
            "zsh loader should refresh completion system after fpath update"
        );
    }

    #[test]
    fn zsh_completion_script_does_not_source_package_completion_files_directly() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut output = Vec::new();
        write_completions_script(CliCompletionShell::Zsh, &layout, &mut output)
            .expect("completion script generation should succeed");
        let rendered = String::from_utf8(output).expect("completion script should be utf-8");

        assert!(
            !rendered.contains("_crosspack_pkg_completion_path"),
            "zsh loader must avoid sourcing completion files directly"
        );
        assert!(
            !rendered.contains("while IFS= read -r"),
            "zsh loader should not use bash-style source loop"
        );
    }

    #[test]
    fn zsh_completion_script_initializes_compinit_before_crosspack_registration() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut output = Vec::new();
        write_completions_script(CliCompletionShell::Zsh, &layout, &mut output)
            .expect("completion script generation should succeed");
        let rendered = String::from_utf8(output).expect("completion script should be utf-8");

        let compinit_index = rendered
            .find("compinit -i")
            .expect("zsh script should initialize compinit");
        let compdef_index = rendered
            .find("compdef _crosspack crosspack")
            .expect("zsh script should register crosspack completion function");

        assert!(
            compinit_index < compdef_index,
            "zsh script must initialize completion system before compdef registration"
        );
    }

    #[test]
    fn parse_provider_overrides_rejects_invalid_shape() {
        let err = parse_provider_overrides(&["missing-equals".to_string()])
            .expect_err("override must require capability=package shape");
        assert!(err.to_string().contains("expected capability=package"));
    }

    #[test]
    fn parse_provider_overrides_rejects_invalid_capability_token() {
        let err = parse_provider_overrides(&["BadCap=clang".to_string()])
            .expect_err("invalid capability token must fail");
        assert!(err.to_string().contains("capability 'BadCap'"));
    }

    #[test]
    fn apply_provider_override_selects_requested_capability_provider() {
        let gcc = PackageManifest::from_toml_str(
            r#"
name = "gcc"
version = "2.0.0"
provides = ["compiler"]
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/gcc-2.0.0.tar.zst"
sha256 = "gcc"
"#,
        )
        .expect("gcc manifest must parse");
        let llvm = PackageManifest::from_toml_str(
            r#"
name = "llvm"
version = "2.1.0"
provides = ["compiler"]
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/llvm-2.1.0.tar.zst"
sha256 = "llvm"
"#,
        )
        .expect("llvm manifest must parse");

        let mut overrides = BTreeMap::new();
        overrides.insert("compiler".to_string(), "llvm".to_string());

        let selected = apply_provider_override("compiler", vec![gcc, llvm], &overrides)
            .expect("provider override must filter candidate set");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "llvm");
    }

    #[test]
    fn apply_provider_override_errors_when_requested_provider_missing() {
        let gcc = PackageManifest::from_toml_str(
            r#"
name = "gcc"
version = "2.0.0"
provides = ["compiler"]
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/gcc-2.0.0.tar.zst"
sha256 = "gcc"
"#,
        )
        .expect("manifest must parse");

        let mut overrides = BTreeMap::new();
        overrides.insert("compiler".to_string(), "clang".to_string());

        let err = apply_provider_override("compiler", vec![gcc], &overrides)
            .expect_err("missing requested provider must fail early");
        assert!(
            err.to_string()
                .contains("provider override 'compiler=clang'"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn apply_provider_override_rejects_overriding_direct_package_tokens() {
        let foo = PackageManifest::from_toml_str(
            r#"
name = "foo"
version = "1.0.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/foo-1.0.0.tar.zst"
sha256 = "foo"
"#,
        )
        .expect("foo manifest must parse");
        let bar = PackageManifest::from_toml_str(
            r#"
name = "bar"
version = "1.0.0"
provides = ["foo"]
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/bar-1.0.0.tar.zst"
sha256 = "bar"
"#,
        )
        .expect("bar manifest must parse");

        let mut overrides = BTreeMap::new();
        overrides.insert("foo".to_string(), "bar".to_string());

        let err = apply_provider_override("foo", vec![foo, bar], &overrides)
            .expect_err("direct package tokens must not be overridable");
        assert!(
            err.to_string()
                .contains("direct package names cannot be overridden"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_provider_overrides_used_accepts_consumed_overrides() {
        let mut overrides = BTreeMap::new();
        overrides.insert("compiler".to_string(), "llvm".to_string());
        overrides.insert("rust-toolchain".to_string(), "rustup".to_string());

        let resolved_dependency_tokens = HashSet::from([
            "compiler".to_string(),
            "rust-toolchain".to_string(),
            "ripgrep".to_string(),
        ]);

        validate_provider_overrides_used(&overrides, &resolved_dependency_tokens)
            .expect("all overrides should be consumed by the resolved graph");
    }

    #[test]
    fn validate_provider_overrides_used_rejects_unused_overrides() {
        let mut overrides = BTreeMap::new();
        overrides.insert("compiler".to_string(), "llvm".to_string());
        overrides.insert("rust-toolchain".to_string(), "rustup".to_string());

        let resolved_dependency_tokens = HashSet::from(["compiler".to_string()]);

        let err = validate_provider_overrides_used(&overrides, &resolved_dependency_tokens)
            .expect_err("unused overrides must fail fast");
        assert!(
            err.to_string()
                .contains("unused provider override(s): rust-toolchain=rustup"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_provider_overrides_used_accepts_union_of_multi_plan_tokens() {
        let mut overrides = BTreeMap::new();
        overrides.insert("compiler".to_string(), "llvm".to_string());
        overrides.insert("rust-toolchain".to_string(), "rustup".to_string());

        let plan_a_tokens = HashSet::from(["compiler".to_string()]);
        let plan_b_tokens = HashSet::from(["rust-toolchain".to_string()]);

        let mut combined_tokens = HashSet::new();
        combined_tokens.extend(plan_a_tokens);
        combined_tokens.extend(plan_b_tokens);

        validate_provider_overrides_used(&overrides, &combined_tokens)
            .expect("overrides consumed across plans should pass");
    }

    #[test]
    fn format_info_lines_includes_policy_sections_when_present() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "compiler"
version = "2.1.0"
provides = ["c-compiler", "cc"]

[conflicts]
legacy-cc = "*"

[replaces]
old-cc = "<2.0.0"
"#,
        )
        .expect("manifest must parse");

        let lines = format_info_lines("compiler", &[manifest]);
        assert_eq!(lines[0], "Package: compiler");
        assert_eq!(lines[1], "- 2.1.0");
        assert_eq!(lines[2], "  Provides: c-compiler, cc");
        assert_eq!(lines[3], "  Conflicts: legacy-cc(*)");
        assert_eq!(lines[4], "  Replaces: old-cc(<2.0.0)");
    }

    #[test]
    fn cli_parses_registry_add_command() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "registry",
            "add",
            "official",
            "https://example.com/official.git",
            "--kind",
            "git",
            "--priority",
            "10",
            "--fingerprint",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::Registry {
                command:
                    super::RegistryCommands::Add {
                        name,
                        location,
                        kind,
                        priority,
                        fingerprint,
                    },
            } => {
                assert_eq!(name, "official");
                assert_eq!(location, "https://example.com/official.git");
                assert_eq!(kind, CliRegistryKind::Git);
                assert_eq!(priority, 10);
                assert_eq!(
                    fingerprint,
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                );
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_registry_remove_with_purge_cache() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "registry",
            "remove",
            "official",
            "--purge-cache",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::Registry {
                command: super::RegistryCommands::Remove { name, purge_cache },
            } => {
                assert_eq!(name, "official");
                assert!(purge_cache);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_registry_list_command() {
        let cli =
            Cli::try_parse_from(["crosspack", "registry", "list"]).expect("command must parse");

        match cli.command {
            Commands::Registry {
                command: super::RegistryCommands::List,
            } => {}
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_rejects_registry_add_without_required_kind_flag() {
        let err = Cli::try_parse_from([
            "crosspack",
            "registry",
            "add",
            "official",
            "https://example.com/official.git",
            "--priority",
            "10",
            "--fingerprint",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ])
        .expect_err("missing --kind should fail");

        let rendered = err.to_string();
        assert!(rendered.contains("--kind <KIND>"));
    }

    #[test]
    fn cli_rejects_registry_add_when_priority_value_missing() {
        let err = Cli::try_parse_from([
            "crosspack",
            "registry",
            "add",
            "official",
            "https://example.com/official.git",
            "--kind",
            "git",
            "--priority",
            "--fingerprint",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ])
        .expect_err("missing --priority value should fail");

        let rendered = err.to_string();
        assert!(rendered.contains("--priority <PRIORITY>"));
    }

    #[test]
    fn cli_rejects_registry_remove_without_name() {
        let err = Cli::try_parse_from(["crosspack", "registry", "remove"])
            .expect_err("missing remove name should fail");

        let rendered = err.to_string();
        assert!(rendered.contains("<NAME>"));
    }

    #[test]
    fn cli_rejects_update_when_registry_value_missing() {
        let err = Cli::try_parse_from(["crosspack", "update", "--registry"])
            .expect_err("missing --registry value should fail");

        let rendered = err.to_string();
        assert!(rendered.contains("a value is required for '--registry <REGISTRY>'"));
    }

    #[test]
    fn cli_parses_update_with_multiple_registry_flags() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "update",
            "--registry",
            "official",
            "--registry",
            "mirror",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::Update { registry } => {
                assert_eq!(registry, vec!["official", "mirror"]);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn registry_list_output_is_sorted() {
        let sources = vec![
            RegistrySourceWithSnapshotState {
                source: RegistrySourceRecord {
                    name: "zeta".to_string(),
                    kind: RegistrySourceKind::Git,
                    location: "https://example.test/zeta.git".to_string(),
                    fingerprint_sha256:
                        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                            .to_string(),
                    enabled: true,
                    priority: 10,
                },
                snapshot: RegistrySourceSnapshotState::Ready {
                    snapshot_id: "git:0123456789abcdef".to_string(),
                },
            },
            RegistrySourceWithSnapshotState {
                source: RegistrySourceRecord {
                    name: "alpha".to_string(),
                    kind: RegistrySourceKind::Filesystem,
                    location: "/tmp/alpha".to_string(),
                    fingerprint_sha256:
                        "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
                            .to_string(),
                    enabled: true,
                    priority: 1,
                },
                snapshot: RegistrySourceSnapshotState::None,
            },
        ];

        let lines = format_registry_list_lines(sources);
        assert_eq!(
            lines[0],
            "alpha kind=filesystem priority=1 location=/tmp/alpha snapshot=none"
        );
        assert_eq!(
            lines[1],
            "zeta kind=git priority=10 location=https://example.test/zeta.git snapshot=ready:git:0123456789abcdef"
        );
    }

    #[test]
    fn format_registry_add_lines_matches_source_management_spec() {
        let lines = format_registry_add_lines(
            "official",
            "git",
            10,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        );

        assert_eq!(
            lines,
            vec![
                "added registry official".to_string(),
                "kind: git".to_string(),
                "priority: 10".to_string(),
                "fingerprint: 0123456789abcdef...".to_string(),
            ]
        );
    }

    #[test]
    fn format_registry_remove_lines_matches_source_management_spec() {
        let lines = format_registry_remove_lines("official", true);
        assert_eq!(lines, vec!["removed registry official", "cache: purged"]);

        let lines = format_registry_remove_lines("official", false);
        assert_eq!(lines, vec!["removed registry official", "cache: kept"]);
    }

    #[test]
    fn format_registry_list_snapshot_error_line_uses_reason_code() {
        let line = format_registry_list_snapshot_state(&RegistrySourceSnapshotState::Error {
            status: RegistrySourceWithSnapshotStatus::Unreadable,
            reason_code: "snapshot-unreadable".to_string(),
        });
        assert_eq!(line, "error:snapshot-unreadable");
    }

    #[test]
    fn resolve_transaction_snapshot_id_ignores_disabled_sources() {
        let layout = test_layout();
        let state_root = registry_state_root(&layout);
        let store = RegistrySourceStore::new(&state_root);
        let snap_root = |name: &str| state_root.join("cache").join(name).join("snapshot.json");

        store
            .add_source(RegistrySourceRecord {
                name: "alpha".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: "/tmp/alpha".to_string(),
                fingerprint_sha256:
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
                enabled: true,
                priority: 1,
            })
            .expect("must add alpha source");
        store
            .add_source(RegistrySourceRecord {
                name: "beta".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: "/tmp/beta".to_string(),
                fingerprint_sha256:
                    "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210".to_string(),
                enabled: false,
                priority: 2,
            })
            .expect("must add beta source");

        std::fs::create_dir_all(state_root.join("cache/alpha"))
            .expect("must create alpha cache directory");
        std::fs::create_dir_all(state_root.join("cache/beta"))
            .expect("must create beta cache directory");
        std::fs::write(
            snap_root("alpha"),
            r#"{"version":1,"source":"alpha","snapshot_id":"snapshot-a","updated_at_unix":1,"manifest_count":0,"status":"ready"}"#,
        )
        .expect("must write alpha snapshot");
        std::fs::write(
            snap_root("beta"),
            r#"{"version":1,"source":"beta","snapshot_id":"snapshot-b","updated_at_unix":1,"manifest_count":0,"status":"ready"}"#,
        )
        .expect("must write beta snapshot");

        let snapshot_id = resolve_transaction_snapshot_id(&layout, "install")
            .expect("must ignore disabled source snapshot");
        assert_eq!(snapshot_id, "snapshot-a");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn resolve_transaction_snapshot_id_rejects_mixed_ready_snapshots() {
        let layout = test_layout();
        let state_root = registry_state_root(&layout);
        let store = RegistrySourceStore::new(&state_root);
        let snap_root = |name: &str| state_root.join("cache").join(name).join("snapshot.json");

        store
            .add_source(RegistrySourceRecord {
                name: "alpha".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: "/tmp/alpha".to_string(),
                fingerprint_sha256:
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
                enabled: true,
                priority: 1,
            })
            .expect("must add alpha source");
        store
            .add_source(RegistrySourceRecord {
                name: "beta".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: "/tmp/beta".to_string(),
                fingerprint_sha256:
                    "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210".to_string(),
                enabled: true,
                priority: 2,
            })
            .expect("must add beta source");

        std::fs::create_dir_all(state_root.join("cache/alpha"))
            .expect("must create alpha cache directory");
        std::fs::create_dir_all(state_root.join("cache/beta"))
            .expect("must create beta cache directory");
        std::fs::write(
            snap_root("alpha"),
            r#"{"version":1,"source":"alpha","snapshot_id":"snapshot-a","updated_at_unix":1,"manifest_count":0,"status":"ready"}"#,
        )
        .expect("must write alpha snapshot");
        std::fs::write(
            snap_root("beta"),
            r#"{"version":1,"source":"beta","snapshot_id":"snapshot-b","updated_at_unix":1,"manifest_count":0,"status":"ready"}"#,
        )
        .expect("must write beta snapshot");

        let err = resolve_transaction_snapshot_id(&layout, "install")
            .expect_err("must fail mixed snapshots");
        let rendered = err.to_string();
        assert!(rendered.contains("metadata snapshot mismatch across configured sources"));
        assert!(rendered.contains("alpha=snapshot-a"));
        assert!(rendered.contains("beta=snapshot-b"));
        let monitor_raw =
            std::fs::read_to_string(layout.transactions_dir().join("snapshot-monitor.log"))
                .expect("must write mismatch telemetry log");
        assert!(monitor_raw.contains("event=snapshot_id_consistency_mismatch"));
        assert!(monitor_raw.contains("error_code=snapshot-id-mismatch"));
        assert!(monitor_raw.contains("operation=install"));
        assert!(monitor_raw.contains("source_count=2"));
        assert!(monitor_raw.contains("unique_snapshot_ids=2"));
        assert!(monitor_raw.contains("sources=alpha=snapshot-a,beta=snapshot-b"));

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn resolve_transaction_snapshot_id_uses_shared_snapshot_id() {
        let layout = test_layout();
        let state_root = registry_state_root(&layout);
        let store = RegistrySourceStore::new(&state_root);
        let snap_root = |name: &str| state_root.join("cache").join(name).join("snapshot.json");

        store
            .add_source(RegistrySourceRecord {
                name: "alpha".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: "/tmp/alpha".to_string(),
                fingerprint_sha256:
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
                enabled: true,
                priority: 1,
            })
            .expect("must add alpha source");
        store
            .add_source(RegistrySourceRecord {
                name: "beta".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: "/tmp/beta".to_string(),
                fingerprint_sha256:
                    "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210".to_string(),
                enabled: true,
                priority: 2,
            })
            .expect("must add beta source");

        std::fs::create_dir_all(state_root.join("cache/alpha"))
            .expect("must create alpha cache directory");
        std::fs::create_dir_all(state_root.join("cache/beta"))
            .expect("must create beta cache directory");
        std::fs::write(
            snap_root("alpha"),
            r#"{"version":1,"source":"alpha","snapshot_id":"snapshot-shared","updated_at_unix":1,"manifest_count":0,"status":"ready"}"#,
        )
        .expect("must write alpha snapshot");
        std::fs::write(
            snap_root("beta"),
            r#"{"version":1,"source":"beta","snapshot_id":"snapshot-shared","updated_at_unix":1,"manifest_count":0,"status":"ready"}"#,
        )
        .expect("must write beta snapshot");

        let snapshot_id = resolve_transaction_snapshot_id(&layout, "upgrade")
            .expect("must choose shared snapshot id");
        assert_eq!(snapshot_id, "snapshot-shared");
        assert!(
            !layout
                .transactions_dir()
                .join("snapshot-monitor.log")
                .exists(),
            "shared snapshot id should not emit mismatch telemetry"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn resolve_transaction_snapshot_id_requires_ready_snapshot() {
        let layout = test_layout();
        let state_root = registry_state_root(&layout);
        let store = RegistrySourceStore::new(&state_root);

        store
            .add_source(RegistrySourceRecord {
                name: "alpha".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: "/tmp/alpha".to_string(),
                fingerprint_sha256:
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
                enabled: true,
                priority: 1,
            })
            .expect("must add alpha source");

        let err = resolve_transaction_snapshot_id(&layout, "install")
            .expect_err("must fail without ready snapshot");
        assert!(err.to_string().contains(
            "no configured registry snapshots available; bootstrap trusted source `core`"
        ));

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_update_command_returns_err_on_partial_failure() {
        let root = test_layout();
        let store = RegistrySourceStore::new(registry_state_root(&root));

        let ok_source = test_registry_source_dir("ok-source", true);
        let bad_source = test_registry_source_dir("bad-source", false);

        store
            .add_source(RegistrySourceRecord {
                name: "ok".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: ok_source.display().to_string(),
                fingerprint_sha256:
                    "f0cf90f634c31f8f43f56f3576d2f23f9f66d4b041e92f788bcbdbdbf4dcd89f".to_string(),
                enabled: true,
                priority: 1,
            })
            .expect("must add ok source");
        store
            .add_source(RegistrySourceRecord {
                name: "bad".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: bad_source.display().to_string(),
                fingerprint_sha256:
                    "f0cf90f634c31f8f43f56f3576d2f23f9f66d4b041e92f788bcbdbdbf4dcd89f".to_string(),
                enabled: true,
                priority: 2,
            })
            .expect("must add bad source");

        let err = run_update_command(&store, &[]).expect_err("partial failure must return err");
        assert_eq!(err.to_string(), "source update failed");

        let _ = std::fs::remove_dir_all(root.prefix());
        let _ = std::fs::remove_dir_all(ok_source);
        let _ = std::fs::remove_dir_all(bad_source);
    }

    #[test]
    fn search_uses_registry_root_override_when_present() {
        let layout = test_layout();
        let override_root = PathBuf::from("/tmp/override-registry");

        let backend = select_metadata_backend(Some(override_root.as_path()), &layout)
            .expect("override backend must resolve");
        assert!(matches!(backend, MetadataBackend::Legacy(_)));
    }

    #[test]
    fn search_uses_configured_sources_without_registry_root() {
        let layout = test_layout();
        let state_root = registry_state_root(&layout);
        std::fs::create_dir_all(state_root.join("cache/official/index/ripgrep"))
            .expect("must create source cache structure");
        std::fs::write(
            state_root.join("sources.toml"),
            concat!(
                "version = 1\n",
                "\n",
                "[[sources]]\n",
                "name = \"official\"\n",
                "kind = \"filesystem\"\n",
                "location = \"/tmp/official\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "enabled = true\n",
                "priority = 1\n"
            ),
        )
        .expect("must write configured sources file");
        std::fs::write(
            state_root.join("cache/official/snapshot.json"),
            r#"{
  "version": 1,
  "source": "official",
  "snapshot_id": "fs:test",
  "updated_at_unix": 1,
  "manifest_count": 0,
  "status": "ready"
}"#,
        )
        .expect("must write snapshot metadata");

        let backend = select_metadata_backend(None, &layout)
            .expect("configured backend must resolve without override");
        assert!(matches!(backend, MetadataBackend::Configured(_)));

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_search_command_formats_exact_prefix_and_keyword_matches_deterministically() {
        let layout = test_layout();
        configure_ready_source(&layout, "official");
        write_signed_test_manifest(
            &layout,
            "official",
            "rip",
            "1.0.1",
            Some("MIT"),
            Some("https://rip.example.test"),
            &[],
        );
        write_signed_test_manifest(
            &layout,
            "official",
            "ripgrep",
            "14.1.0",
            None,
            None,
            &["rg"],
        );
        write_signed_test_manifest(&layout, "official", "roundrip", "0.9.0", None, None, &[]);

        let backend = select_metadata_backend(None, &layout).expect("configured backend must load");
        let results = run_search_command(&backend, "rip").expect("search must succeed");
        let lines = format_search_results(&results, "rip");

        assert_eq!(
            lines,
            vec![
                "name\tdescription\tlatest\tsource".to_string(),
                "rip\tlicense: MIT\t1.0.1\tofficial".to_string(),
                "ripgrep\tprovides: rg\t14.1.0\tofficial".to_string(),
                "roundrip\t-\t0.9.0\tofficial".to_string(),
            ]
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn format_search_results_reports_empty_match_with_actionable_guidance() {
        let lines = format_search_results(&[], "rip");

        assert_eq!(
            lines,
            vec![
                "No packages found matching 'rip'. Try a broader keyword or run `crosspack update` to refresh local snapshots."
                    .to_string(),
            ]
        );
    }

    #[test]
    fn run_search_command_returns_actionable_guidance_when_source_metadata_is_unavailable() {
        let layout = test_layout();
        configure_ready_source(&layout, "official");
        std::fs::create_dir_all(
            registry_state_root(&layout)
                .join("cache")
                .join("official")
                .join("index")
                .join("ripgrep"),
        )
        .expect("must create package directory");

        let backend = select_metadata_backend(None, &layout).expect("configured backend must load");
        let err = run_search_command(&backend, "rip").expect_err("missing registry key must fail");
        let rendered = err.to_string();
        assert!(rendered.contains("search metadata unavailable"));
        assert!(rendered.contains("crosspack update"));
        assert!(rendered.contains("crosspack registry list"));

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn metadata_commands_fail_with_guidance_when_no_sources_or_snapshots() {
        let layout = test_layout();

        let err = select_metadata_backend(None, &layout)
            .expect_err("must fail when no configured metadata backend is available");
        let rendered = err.to_string();
        assert!(rendered.contains("crosspack registry add"));
        assert!(rendered.contains("crosspack update"));
    }

    #[test]
    fn update_failure_reason_code_prefers_deterministic_reason_prefix() {
        let reason = update_failure_reason_code(Some(
            "source-sync-failed: source 'official' git fetch failed: fatal: bad object",
        ));
        assert_eq!(reason, "source-sync-failed");
    }

    #[test]
    fn update_failure_reason_code_falls_back_to_unknown_for_unstructured_error() {
        let reason = update_failure_reason_code(Some("failed to sync source with weird error"));
        assert_eq!(reason, "unknown");
    }

    #[test]
    fn build_update_report_formats_failed_result_with_reason_code_only() {
        let results = vec![SourceUpdateResult {
            name: "official".to_string(),
            status: SourceUpdateStatus::Failed,
            snapshot_id: String::new(),
            error: Some(
                "source-metadata-invalid: source 'official' package 'ripgrep' failed signature validation: nested detail"
                    .to_string(),
            ),
        }];

        let report = build_update_report(&results);
        assert_eq!(
            report.lines,
            vec!["official: failed (reason=source-metadata-invalid)"]
        );
        assert_eq!(report.failed, 1);
    }

    #[test]
    fn ensure_update_succeeded_returns_err_when_any_source_failed() {
        let err = ensure_update_succeeded(1).expect_err("must return err when failures exist");
        assert_eq!(err.to_string(), "source update failed");
    }

    #[test]
    fn format_update_summary_line_matches_contract() {
        let line = format_update_summary_line(2, 5, 1);
        assert_eq!(line, "update summary: updated=2 up-to-date=5 failed=1");
    }

    #[test]
    fn resolve_output_style_auto_uses_rich_when_both_streams_are_tty() {
        assert_eq!(resolve_output_style(true, true), OutputStyle::Rich);
    }

    #[test]
    fn resolve_output_style_auto_uses_rich_when_stdout_is_tty_and_stderr_is_not() {
        assert_eq!(resolve_output_style(true, false), OutputStyle::Rich);
    }

    #[test]
    fn resolve_output_style_auto_uses_plain_when_stdout_is_not_tty() {
        assert_eq!(resolve_output_style(false, true), OutputStyle::Plain);
    }

    #[test]
    fn resolve_install_progress_mode_disables_progress_for_plain_output() {
        assert_eq!(
            resolve_install_progress_mode(OutputStyle::Plain, Some("en_US.UTF-8")),
            InstallProgressMode::Disabled
        );
    }

    #[test]
    fn resolve_install_progress_mode_prefers_unicode_for_utf8_locale() {
        assert_eq!(
            resolve_install_progress_mode(OutputStyle::Rich, Some("en_US.UTF-8")),
            InstallProgressMode::Unicode
        );
    }

    #[test]
    fn resolve_install_progress_mode_falls_back_to_ascii_for_non_utf8_locale() {
        assert_eq!(
            resolve_install_progress_mode(OutputStyle::Rich, Some("C")),
            InstallProgressMode::Ascii
        );
    }

    #[test]
    fn format_install_progress_line_uses_ascii_spinner_and_progress_bar() {
        let line = format_install_progress_line(
            InstallProgressMode::Ascii,
            1,
            "install",
            "gh",
            InstallProgressLineState {
                phase: "download",
                step: 2,
                total_steps: 6,
                download_progress: None,
            },
        );

        assert!(line.starts_with("\\ install gh"), "unexpected line: {line}");
        assert!(line.contains("download"), "unexpected line: {line}");
        assert!(line.contains("2/6"), "unexpected line: {line}");
        assert!(line.contains("["), "unexpected line: {line}");
        assert!(line.contains("]"), "unexpected line: {line}");
    }

    #[test]
    fn format_install_progress_line_uses_unicode_spinner_when_enabled() {
        let line = format_install_progress_line(
            InstallProgressMode::Unicode,
            0,
            "install",
            "gh",
            InstallProgressLineState {
                phase: "prepare",
                step: 1,
                total_steps: 6,
                download_progress: None,
            },
        );

        assert!(
            line.starts_with("\u{280b} install gh"),
            "unexpected line: {line}"
        );
        assert!(line.contains("prepare"), "unexpected line: {line}");
    }

    #[test]
    fn format_install_progress_line_includes_percent_when_total_is_known() {
        let line = format_install_progress_line(
            InstallProgressMode::Ascii,
            0,
            "install",
            "gh",
            InstallProgressLineState {
                phase: "download",
                step: 2,
                total_steps: 7,
                download_progress: Some((50, Some(200))),
            },
        );

        assert!(line.contains("50B/200B (25%)"), "unexpected line: {line}");
    }

    #[test]
    fn format_install_progress_line_uses_download_fraction_for_known_total_bar_fill() {
        let line = format_install_progress_line(
            InstallProgressMode::Ascii,
            0,
            "install",
            "gh",
            InstallProgressLineState {
                phase: "download",
                step: 2,
                total_steps: 7,
                download_progress: Some((50, Some(200))),
            },
        );

        assert!(
            line.contains("[=====---------------]"),
            "unexpected line: {line}"
        );
        assert!(line.contains("2/7 download"), "unexpected line: {line}");
    }

    #[test]
    fn format_install_progress_line_uses_indeterminate_download_bar_when_total_unknown() {
        let extract_bar = |line: &str| {
            let start = line
                .find('[')
                .expect("line must contain opening bar bracket");
            let end = line
                .find(']')
                .expect("line must contain closing bar bracket");
            line[start + 1..end].to_string()
        };
        let line_frame_0 = format_install_progress_line(
            InstallProgressMode::Ascii,
            0,
            "install",
            "gh",
            InstallProgressLineState {
                phase: "download",
                step: 2,
                total_steps: 7,
                download_progress: Some((50, None)),
            },
        );
        let line_frame_1 = format_install_progress_line(
            InstallProgressMode::Ascii,
            12,
            "install",
            "gh",
            InstallProgressLineState {
                phase: "download",
                step: 2,
                total_steps: 7,
                download_progress: Some((50, None)),
            },
        );

        assert_ne!(
            extract_bar(&line_frame_0),
            extract_bar(&line_frame_1),
            "bars should animate by frame beyond spinner positions"
        );
        assert!(
            line_frame_0.contains("50B"),
            "unexpected line: {line_frame_0}"
        );
        assert!(
            line_frame_1.contains("50B"),
            "unexpected line: {line_frame_1}"
        );
    }

    #[test]
    fn install_progress_renderer_keeps_monotonic_frame_index() {
        let mut renderer =
            InstallProgressRenderer::new(InstallProgressMode::Ascii, "install", "gh", 7);
        let updates = install_progress_frames(InstallProgressMode::Ascii).len() + 8;

        for _ in 0..updates {
            renderer.update("verify", 3, None);
        }

        assert_eq!(renderer.frame_index, updates);
    }

    #[test]
    fn format_install_progress_line_uses_step_progress_for_non_download_phases() {
        let line = format_install_progress_line(
            InstallProgressMode::Ascii,
            0,
            "install",
            "gh",
            InstallProgressLineState {
                phase: "verify",
                step: 2,
                total_steps: 6,
                download_progress: Some((50, Some(200))),
            },
        );

        assert!(
            line.contains("[=======-------------]"),
            "unexpected line: {line}"
        );
        assert!(line.contains("50B/200B (25%)"), "unexpected line: {line}");
    }

    #[test]
    fn format_install_progress_line_shows_bytes_only_when_total_unknown() {
        let line = format_install_progress_line(
            InstallProgressMode::Ascii,
            0,
            "install",
            "gh",
            InstallProgressLineState {
                phase: "download",
                step: 2,
                total_steps: 7,
                download_progress: Some((50, None)),
            },
        );

        assert!(line.contains("50B"), "unexpected line: {line}");
        assert!(!line.contains('%'), "unexpected line: {line}");
    }

    #[test]
    fn format_install_progress_line_shows_zero_bytes_at_download_start() {
        let line = format_install_progress_line(
            InstallProgressMode::Ascii,
            0,
            "install",
            "gh",
            InstallProgressLineState {
                phase: "download",
                step: 2,
                total_steps: 7,
                download_progress: Some((0, None)),
            },
        );

        assert!(line.contains("0B"), "unexpected line: {line}");
        assert!(line.contains("2/7 download"), "unexpected line: {line}");
    }

    #[test]
    fn install_progress_renderer_finish_sequence_keeps_completed_line_visible() {
        let sequence = install_progress_renderer_finish_sequence(true);
        assert_eq!(sequence, "\n");
    }

    #[test]
    fn install_progress_renderer_finish_sequence_clears_incomplete_line() {
        let sequence = install_progress_renderer_finish_sequence(false);
        assert_eq!(sequence, "\r\x1b[2K");
    }

    #[test]
    fn install_progress_throttle_decision_throttles_download_same_step_within_interval() {
        let should_render = should_render_install_progress_update(
            Some("download"),
            Some(2),
            "download",
            2,
            Some(std::time::Duration::from_millis(40)),
        );

        assert!(!should_render, "download redraw should be throttled");
    }

    #[test]
    fn install_progress_throttle_decision_allows_download_when_step_changes() {
        let should_render = should_render_install_progress_update(
            Some("download"),
            Some(1),
            "download",
            2,
            Some(std::time::Duration::from_millis(1)),
        );

        assert!(should_render, "step change should bypass throttling");
    }

    #[test]
    fn install_progress_throttle_decision_keeps_non_download_immediate() {
        let should_render = should_render_install_progress_update(
            Some("verify"),
            Some(3),
            "verify",
            3,
            Some(std::time::Duration::from_millis(1)),
        );

        assert!(should_render, "non-download redraw should remain immediate");
    }

    #[test]
    fn download_artifact_reports_progress_with_known_total() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let payload = b"crosspack-progress-known-total".to_vec();
        let (url, server) = start_one_shot_http_server(payload.clone(), true);
        let cache_path = layout.prefix().join("download-known.bin");
        let mut observed = Vec::new();

        let status =
            download_artifact_with_progress(&url, &cache_path, false, |downloaded, total| {
                observed.push((downloaded, total));
            })
            .expect("download must succeed");

        server.join().expect("server thread must join");

        assert_eq!(status, "downloaded");
        assert_eq!(
            std::fs::read(&cache_path).expect("must read cache file"),
            payload
        );
        assert!(!observed.is_empty(), "progress callback must be invoked");
        let last = observed.last().expect("must have progress events");
        assert_eq!(last.0, payload.len() as u64);
        assert_eq!(last.1, Some(payload.len() as u64));

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn download_artifact_reports_progress_without_total_for_streamed_response() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let payload = b"crosspack-progress-unknown-total".to_vec();
        let (url, server) = start_one_shot_http_server(payload.clone(), false);
        let cache_path = layout.prefix().join("download-unknown.bin");
        let mut observed = Vec::new();

        let status =
            download_artifact_with_progress(&url, &cache_path, false, |downloaded, total| {
                observed.push((downloaded, total));
            })
            .expect("download must succeed");

        server.join().expect("server thread must join");

        assert_eq!(status, "downloaded");
        assert_eq!(
            std::fs::read(&cache_path).expect("must read cache file"),
            payload
        );
        assert!(!observed.is_empty(), "progress callback must be invoked");
        let last = observed.last().expect("must have progress events");
        assert_eq!(last.0, payload.len() as u64);
        assert_eq!(last.1, None);

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn parse_download_backend_preference_defaults_to_in_process() {
        let backend = parse_download_backend_preference(None, "CROSSPACK_DOWNLOAD_BACKEND")
            .expect("empty backend preference should use in-process downloader");
        assert_eq!(backend, DownloadBackendPreference::InProcess);
    }

    #[test]
    fn parse_download_backend_preference_accepts_external_value() {
        let backend =
            parse_download_backend_preference(Some("external"), "CROSSPACK_DOWNLOAD_BACKEND")
                .expect("external backend preference should be accepted");
        assert_eq!(backend, DownloadBackendPreference::External);
    }

    #[test]
    fn parse_download_backend_preference_rejects_unknown_value() {
        let err = parse_download_backend_preference(Some("curl"), "CROSSPACK_DOWNLOAD_BACKEND")
            .expect_err("unknown backend value should fail");
        assert!(
            err.to_string()
                .contains("invalid CROSSPACK_DOWNLOAD_BACKEND value 'curl'"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn download_artifact_cache_hit_ignores_invalid_backend_env() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let cache_path = layout.prefix().join("download-cache-hit.bin");
        std::fs::write(&cache_path, b"cached").expect("must write cache fixture");

        let previous = std::env::var("CROSSPACK_DOWNLOAD_BACKEND").ok();
        unsafe {
            std::env::set_var("CROSSPACK_DOWNLOAD_BACKEND", "not-a-backend");
        }

        let status = download_artifact_with_progress(
            "https://example.test/cached.bin",
            &cache_path,
            false,
            |_downloaded, _total| {},
        )
        .expect("cache hit should short-circuit before backend validation");

        match previous {
            Some(value) => unsafe { std::env::set_var("CROSSPACK_DOWNLOAD_BACKEND", value) },
            None => unsafe { std::env::remove_var("CROSSPACK_DOWNLOAD_BACKEND") },
        }

        assert_eq!(status, "cache-hit");
        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn download_artifact_retries_in_process_download_before_succeeding() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let payload = b"crosspack-retry-success".to_vec();
        let (url, server) = start_retry_http_server(payload.clone(), 3);
        let cache_path = layout.prefix().join("download-retry.bin");

        let status =
            download_artifact_with_progress(&url, &cache_path, false, |_downloaded, _total| {})
                .expect("download must succeed after bounded retries");

        let observed_attempts = server.join().expect("server thread must join");

        assert_eq!(status, "downloaded");
        assert_eq!(
            std::fs::read(&cache_path).expect("must read cache file"),
            payload
        );
        assert_eq!(observed_attempts, 3, "in-process retries should be bounded");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn backend_selection_external_uses_external_downloader_only() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let cache_path = layout.prefix().join("external-only.bin");
        let in_process_calls = std::cell::Cell::new(0_usize);
        let external_calls = std::cell::Cell::new(0_usize);

        let status = download_artifact_with_progress_using(
            "https://example.test/external-only.bin",
            &cache_path,
            false,
            DownloadBackendPreference::External,
            |_downloaded, _total| {},
            |_, _, _| {
                in_process_calls.set(in_process_calls.get() + 1);
                Err(anyhow!("in-process backend must not be used"))
            },
            |_, out_path| {
                external_calls.set(external_calls.get() + 1);
                std::fs::write(out_path, b"external-only").expect("must write external fixture");
                Ok(())
            },
        )
        .expect("external backend should succeed");

        assert_eq!(status, "downloaded");
        assert_eq!(in_process_calls.get(), 0);
        assert_eq!(external_calls.get(), 1);

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn in_process_failure_falls_back_to_external_backend() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let cache_path = layout.prefix().join("in-process-fallback.bin");
        let in_process_calls = std::cell::Cell::new(0_usize);
        let external_calls = std::cell::Cell::new(0_usize);
        let progress_events = std::cell::RefCell::new(Vec::new());

        let status = download_artifact_with_progress_using(
            "https://example.test/in-process-fallback.bin",
            &cache_path,
            false,
            DownloadBackendPreference::InProcess,
            |downloaded, total| {
                progress_events.borrow_mut().push((downloaded, total));
            },
            |_, _, _| {
                in_process_calls.set(in_process_calls.get() + 1);
                Err(anyhow!("simulated in-process failure"))
            },
            |_, out_path| {
                external_calls.set(external_calls.get() + 1);
                std::fs::write(out_path, b"external-fallback")
                    .expect("must write fallback fixture");
                Ok(())
            },
        )
        .expect("external fallback should recover in-process failure");

        assert_eq!(status, "downloaded");
        assert_eq!(in_process_calls.get(), 1);
        assert_eq!(external_calls.get(), 1);
        assert_eq!(
            progress_events.borrow().first().copied(),
            Some((0, None)),
            "download phase should be visible before fallback backend work"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn render_status_line_plain_is_unadorned() {
        assert_eq!(
            render_status_line(OutputStyle::Plain, "ok", "installed ripgrep 14.1.0"),
            "installed ripgrep 14.1.0"
        );
    }

    #[test]
    fn render_status_line_rich_includes_ascii_badge() {
        assert_eq!(
            render_status_line(OutputStyle::Rich, "ok", "installed ripgrep 14.1.0"),
            "[OK] installed ripgrep 14.1.0"
        );
    }

    #[test]
    fn render_status_line_rich_formats_warning() {
        assert_eq!(
            render_status_line(OutputStyle::Rich, "warn", "completion sync skipped"),
            "[WARN] completion sync skipped"
        );
    }

    #[test]
    fn format_update_output_lines_plain_preserves_contract_lines() {
        let report = sample_update_report();
        assert_eq!(
            format_update_output_lines(&report, OutputStyle::Plain),
            report.lines
        );
    }

    #[test]
    fn format_update_output_lines_rich_adds_status_badges() {
        let report = sample_update_report();
        let lines = format_update_output_lines(&report, OutputStyle::Rich);
        assert_eq!(lines[0], "[OK] core: updated (snapshot=git:abc)");
        assert_eq!(lines[1], "[..] mirror: up-to-date (snapshot=git:abc)");
        assert_eq!(lines[2], "[ERR] edge: failed (reason=source-sync-failed)");
    }

    #[test]
    fn format_install_outcome_lines_plain_matches_existing_contract() {
        let outcome = sample_install_outcome();
        let lines = format_install_outcome_lines(&outcome, OutputStyle::Plain);
        assert_eq!(
            lines[0],
            "resolved ripgrep 14.1.0 for x86_64-unknown-linux-gnu"
        );
        assert_eq!(lines[1], "archive: tar.zst");
    }

    #[test]
    fn format_install_outcome_lines_rich_adds_step_indicators() {
        let outcome = sample_install_outcome();
        let lines = format_install_outcome_lines(&outcome, OutputStyle::Rich);
        assert!(lines[0].starts_with("[OK] "));
        assert!(lines.iter().any(|line| line.contains("receipt: ")));
    }

    #[test]
    fn install_resolved_emits_warning_when_native_gui_registration_fails() {
        let mut outcome = sample_install_outcome();
        outcome.warnings = vec!["native registration skipped".to_string()];

        let lines = format_install_outcome_lines(&outcome, OutputStyle::Plain);
        assert!(
            lines
                .iter()
                .any(|line| line == "warning: native registration skipped"),
            "install output must include native GUI warning lines"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn install_reports_actionable_error_for_unsupported_exe_host() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut resolved = resolved_install("demo-exe", "1.0.0");
        resolved.archive_type = ArchiveType::Exe;
        resolved.artifact.url = "https://example.test/demo-exe-1.0.0.exe".to_string();
        resolved.artifact.sha256 = EMPTY_SHA256.to_string();

        seed_cached_artifact(&layout, &resolved, b"");

        let err = install_resolved(
            &layout,
            &resolved,
            &[],
            &[],
            &HashMap::new(),
            InstallResolvedOptions {
                snapshot_id: None,
                force_redownload: false,
                interaction_policy: InstallInteractionPolicy::default(),
                install_progress_mode: InstallProgressMode::Disabled,
            },
        )
        .expect_err("unsupported EXE host should fail deterministically");

        assert!(
            err.to_string()
                .contains("EXE artifacts are supported only on Windows hosts"),
            "unexpected error: {err}"
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn install_reports_actionable_error_for_unsupported_pkg_host() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut resolved = resolved_install("demo-pkg", "1.0.0");
        resolved.archive_type = ArchiveType::Pkg;
        resolved.artifact.url = "https://example.test/demo-pkg-1.0.0.pkg".to_string();
        resolved.artifact.sha256 = EMPTY_SHA256.to_string();

        seed_cached_artifact(&layout, &resolved, b"");

        let err = install_resolved(
            &layout,
            &resolved,
            &[],
            &[],
            &HashMap::new(),
            InstallResolvedOptions {
                snapshot_id: None,
                force_redownload: false,
                interaction_policy: InstallInteractionPolicy::default(),
                install_progress_mode: InstallProgressMode::Disabled,
            },
        )
        .expect_err("unsupported PKG host should fail deterministically");

        assert!(
            err.to_string()
                .contains("PKG artifacts are supported only on macOS hosts"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn native_gui_sync_contract_accepts_previous_registration_records() {
        type RegisterNativeGuiFn = fn(
            &str,
            &ArtifactGuiApp,
            &Path,
            &[GuiNativeRegistrationRecord],
        )
            -> Result<(Vec<GuiNativeRegistrationRecord>, Vec<String>)>;

        let _register: RegisterNativeGuiFn = register_native_gui_app_best_effort;
    }

    #[test]
    fn native_gui_sync_same_path_kind_migration_keeps_deployed_bundle_copy() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let deployed_bundle = layout.prefix().join("Applications").join("Demo.app");
        let deployed_binary = deployed_bundle.join("Contents").join("MacOS").join("demo");
        fs::create_dir_all(deployed_binary.parent().expect("must have parent"))
            .expect("must create deployed bundle dirs");
        fs::write(&deployed_binary, b"#!/bin/sh\n").expect("must create deployed bundle binary");

        let previous = vec![GuiNativeRegistrationRecord {
            key: "app:demo".to_string(),
            kind: "applications-symlink".to_string(),
            path: deployed_bundle.display().to_string(),
        }];
        let current = vec![GuiNativeRegistrationRecord {
            key: "app:demo".to_string(),
            kind: "applications-bundle-copy".to_string(),
            path: deployed_bundle.display().to_string(),
        }];

        let stale = select_stale_native_gui_registration_records(&previous, &current);
        assert!(
            stale.is_empty(),
            "same-path kind migration must not schedule stale cleanup"
        );
        let warnings = remove_native_gui_registration_best_effort(&stale)
            .expect("empty stale cleanup should be a no-op");
        assert!(warnings.is_empty(), "no-op cleanup should not warn");
        assert!(deployed_bundle.exists(), "deployed bundle copy must remain");
        assert!(
            deployed_binary.exists(),
            "deployed bundle binary must remain"
        );

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn native_gui_sync_kind_migration_with_path_change_preserves_legacy_bundle_dir() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let stale_bundle = layout.prefix().join("Applications").join("OldDemo.app");
        let stale_binary = stale_bundle.join("Contents").join("MacOS").join("demo");
        fs::create_dir_all(stale_binary.parent().expect("must have parent"))
            .expect("must create stale bundle dirs");
        fs::write(&stale_binary, b"#!/bin/sh\n").expect("must create stale bundle binary");

        let deployed_bundle = layout.prefix().join("Applications").join("Demo.app");
        let deployed_binary = deployed_bundle.join("Contents").join("MacOS").join("demo");
        fs::create_dir_all(deployed_binary.parent().expect("must have parent"))
            .expect("must create deployed bundle dirs");
        fs::write(&deployed_binary, b"#!/bin/sh\n").expect("must create deployed bundle binary");

        let previous = vec![GuiNativeRegistrationRecord {
            key: "app:demo".to_string(),
            kind: "applications-symlink".to_string(),
            path: stale_bundle.display().to_string(),
        }];
        let current = vec![GuiNativeRegistrationRecord {
            key: "app:demo".to_string(),
            kind: "applications-bundle-copy".to_string(),
            path: deployed_bundle.display().to_string(),
        }];

        let stale = select_stale_native_gui_registration_records(&previous, &current);
        assert_eq!(stale, previous, "path change must remain stale");
        let warnings = remove_native_gui_registration_best_effort(&stale)
            .expect("stale cleanup should succeed");
        assert!(warnings.is_empty(), "stale cleanup should be warning-free");
        assert!(
            stale_bundle.exists(),
            "legacy applications-symlink bundle dirs should not be removed recursively"
        );
        assert!(deployed_bundle.exists(), "deployed bundle path must remain");
        assert!(
            deployed_binary.exists(),
            "deployed bundle binary must remain"
        );

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn upgrade_removes_stale_native_gui_records() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let install_root = layout.package_dir("demo", "1.0.0");
        fs::create_dir_all(&install_root).expect("must create install root");
        let stale_path = layout.prefix().join("stale-native.desktop");
        fs::write(&stale_path, b"stale").expect("must seed stale native file");

        write_gui_native_state(
            &layout,
            "demo",
            &[GuiNativeRegistrationRecord {
                key: "app:demo".to_string(),
                kind: "desktop-entry".to_string(),
                path: stale_path.display().to_string(),
            }],
        )
        .expect("must seed stale native state");

        let (records, warnings) =
            sync_native_gui_registration_state_best_effort(&layout, "demo", &install_root, &[])
                .expect("must sync native state");
        assert!(records.is_empty());
        assert!(
            read_gui_native_state(&layout, "demo")
                .expect("must read state")
                .is_empty(),
            "stale native state should be cleared"
        );
        assert!(warnings.is_empty(), "stale cleanup should be warning-free");
    }

    #[test]
    fn upgrade_preserves_stale_native_gui_records_when_cleanup_warns() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let install_root = layout.package_dir("demo", "1.0.0");
        fs::create_dir_all(&install_root).expect("must create install root");

        let stale = GuiNativeRegistrationRecord {
            key: "app:demo".to_string(),
            kind: "unknown-kind".to_string(),
            path: "/tmp/demo".to_string(),
        };
        write_gui_native_state(&layout, "demo", std::slice::from_ref(&stale))
            .expect("must seed stale native state");

        let (_records, warnings) =
            sync_native_gui_registration_state_best_effort(&layout, "demo", &install_root, &[])
                .expect("must sync native state");
        assert!(!warnings.is_empty());
        assert_eq!(
            read_gui_native_state(&layout, "demo").expect("must read state"),
            vec![stale]
        );
    }

    #[test]
    fn uninstall_removes_native_gui_registrations_and_state() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let package_dir = layout.package_dir("demo", "1.0.0");
        fs::create_dir_all(&package_dir).expect("must create package dir");
        fs::write(package_dir.join("demo"), b"#!/bin/sh\n").expect("must write package binary");
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
        let native_path = layout.prefix().join("demo-native.desktop");
        fs::write(&native_path, b"demo").expect("must write native registration file");
        write_gui_native_state(
            &layout,
            "demo",
            &[GuiNativeRegistrationRecord {
                key: "app:demo".to_string(),
                kind: "desktop-entry".to_string(),
                path: native_path.display().to_string(),
            }],
        )
        .expect("must write native state");

        run_uninstall_command(&layout, "demo".to_string()).expect("must uninstall package");

        assert!(!layout.gui_native_state_path("demo").exists());
    }

    fn resolved_install(name: &str, version: &str) -> ResolvedInstall {
        let manifest = PackageManifest::from_toml_str(&format!(
            r#"
name = "{name}"
version = "{version}"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/{name}-{version}.tar.zst"
sha256 = "abc"
"#
        ))
        .expect("manifest parse");
        let artifact = manifest.artifacts[0].clone();

        ResolvedInstall {
            manifest,
            artifact,
            resolved_target: "x86_64-unknown-linux-gnu".to_string(),
            archive_type: ArchiveType::TarZst,
        }
    }

    fn seed_cached_artifact(layout: &PrefixLayout, resolved: &ResolvedInstall, payload: &[u8]) {
        let cache_path = layout.artifact_cache_path(
            &resolved.manifest.name,
            &resolved.manifest.version.to_string(),
            &resolved.resolved_target,
            resolved.archive_type,
        );
        std::fs::create_dir_all(cache_path.parent().expect("cache path must have parent"))
            .expect("must create cache dir");
        std::fs::write(cache_path, payload).expect("must seed cached artifact");
    }

    fn start_one_shot_http_server(
        payload: Vec<u8>,
        with_content_length: bool,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("must bind one-shot test server");
        let address = listener
            .local_addr()
            .expect("must read one-shot test server address");
        let url = format!("http://{address}/artifact.bin");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("must accept test client");
            let mut request_buffer = [0_u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut request_buffer);

            if with_content_length {
                std::io::Write::write_all(
                    &mut stream,
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        payload.len()
                    )
                    .as_bytes(),
                )
                .expect("must write test response headers");
            } else {
                std::io::Write::write_all(
                    &mut stream,
                    b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n",
                )
                .expect("must write test response headers");
            }
            std::io::Write::write_all(&mut stream, &payload)
                .expect("must write test response payload");
            std::io::Write::flush(&mut stream).expect("must flush test response payload");
        });

        (url, handle)
    }

    fn start_retry_http_server(
        payload: Vec<u8>,
        success_on_attempt: usize,
    ) -> (String, std::thread::JoinHandle<usize>) {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("must bind retry test server");
        let address = listener
            .local_addr()
            .expect("must read retry test server address");
        let url = format!("http://{address}/artifact.bin");
        let handle = std::thread::spawn(move || {
            for attempt in 1..=success_on_attempt {
                let (mut stream, _) = listener.accept().expect("must accept retry test client");
                let mut request_buffer = [0_u8; 1024];
                let _ = std::io::Read::read(&mut stream, &mut request_buffer);

                if attempt < success_on_attempt {
                    std::io::Write::write_all(
                        &mut stream,
                        b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .expect("must write retry test failure response");
                } else {
                    std::io::Write::write_all(
                        &mut stream,
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            payload.len()
                        )
                        .as_bytes(),
                    )
                    .expect("must write retry test success headers");
                    std::io::Write::write_all(&mut stream, &payload)
                        .expect("must write retry test payload");
                }
                std::io::Write::flush(&mut stream).expect("must flush retry test response");
            }

            success_on_attempt
        });

        (url, handle)
    }

    fn sample_install_outcome() -> super::InstallOutcome {
        super::InstallOutcome {
            name: "ripgrep".to_string(),
            version: "14.1.0".to_string(),
            resolved_target: "x86_64-unknown-linux-gnu".to_string(),
            archive_type: ArchiveType::TarZst,
            artifact_url: "https://example.test/ripgrep-14.1.0.tar.zst".to_string(),
            cache_path: PathBuf::from("/tmp/crosspack/cache/ripgrep/14.1.0/artifact.tar.zst"),
            download_status: "downloaded",
            install_root: PathBuf::from("/tmp/crosspack/pkgs/ripgrep/14.1.0"),
            receipt_path: PathBuf::from("/tmp/crosspack/state/installed/ripgrep.receipt"),
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: vec!["bash:rg".to_string()],
            exposed_gui_assets: vec!["app:dev.ripgrep.viewer".to_string()],
            native_gui_records: vec!["app:dev.ripgrep.viewer".to_string()],
            warnings: Vec::new(),
        }
    }

    fn sample_update_report() -> super::UpdateReport {
        super::UpdateReport {
            lines: vec![
                "core: updated (snapshot=git:abc)".to_string(),
                "mirror: up-to-date (snapshot=git:abc)".to_string(),
                "edge: failed (reason=source-sync-failed)".to_string(),
            ],
            updated: 1,
            up_to_date: 1,
            failed: 1,
        }
    }

    static TEST_LAYOUT_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn build_test_layout_path(nanos: u128) -> PathBuf {
        let mut path = std::env::temp_dir();
        let sequence = TEST_LAYOUT_COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!(
            "crosspack-cli-tests-{}-{}-{}",
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
            "test layout paths must remain unique when timestamp granularity is coarse"
        );
    }

    fn test_layout() -> PrefixLayout {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        PrefixLayout::new(build_test_layout_path(nanos))
    }

    fn test_registry_source_dir(name: &str, with_registry_pub: bool) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        path.push(format!("crosspack-cli-test-registry-{name}-{nanos}"));
        std::fs::create_dir_all(path.join("index")).expect("must create index dir");
        if with_registry_pub {
            std::fs::write(path.join("registry.pub"), "test-key\n")
                .expect("must write registry key");
        }
        path
    }

    fn configure_ready_source(layout: &PrefixLayout, source_name: &str) {
        let state_root = registry_state_root(layout);
        std::fs::create_dir_all(state_root.join("cache").join(source_name))
            .expect("must create source cache root");
        std::fs::write(
            state_root.join("sources.toml"),
            format!(
                "version = 1\n\n[[sources]]\nname = \"{source_name}\"\nkind = \"filesystem\"\nlocation = \"/tmp/{source_name}\"\nfingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\nenabled = true\npriority = 1\n"
            ),
        )
        .expect("must write source state");
        std::fs::write(
            state_root.join("cache").join(source_name).join("snapshot.json"),
            format!(
                "{{\n  \"version\": 1,\n  \"source\": \"{source_name}\",\n  \"snapshot_id\": \"fs:test\",\n  \"updated_at_unix\": 1,\n  \"manifest_count\": 0,\n  \"status\": \"ready\"\n}}"
            ),
        )
        .expect("must write snapshot state");
    }

    fn write_signed_test_manifest(
        layout: &PrefixLayout,
        source_name: &str,
        package_name: &str,
        version: &str,
        license: Option<&str>,
        homepage: Option<&str>,
        provides: &[&str],
    ) {
        let cache_root = registry_state_root(layout).join("cache").join(source_name);
        let package_dir = cache_root.join("index").join(package_name);
        std::fs::create_dir_all(&package_dir).expect("must create package directory");

        let signing_key = test_signing_key();
        std::fs::write(
            cache_root.join("registry.pub"),
            public_key_hex(&signing_key),
        )
        .expect("must write registry key");

        let manifest = manifest_toml(package_name, version, license, homepage, provides);
        let manifest_path = package_dir.join(format!("{version}.toml"));
        std::fs::write(&manifest_path, manifest.as_bytes()).expect("must write manifest");

        let signature = signing_key.sign(manifest.as_bytes());
        std::fs::write(
            manifest_path.with_extension("toml.sig"),
            hex::encode(signature.to_bytes()),
        )
        .expect("must write signature");
    }

    fn test_signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7_u8; 32])
    }

    fn public_key_hex(key: &SigningKey) -> String {
        hex::encode(key.verifying_key().to_bytes())
    }

    fn manifest_toml(
        package_name: &str,
        version: &str,
        license: Option<&str>,
        homepage: Option<&str>,
        provides: &[&str],
    ) -> String {
        let mut manifest = format!("name = \"{package_name}\"\nversion = \"{version}\"\n");
        if let Some(license) = license {
            manifest.push_str(&format!("license = \"{license}\"\n"));
        }
        if let Some(homepage) = homepage {
            manifest.push_str(&format!("homepage = \"{homepage}\"\n"));
        }
        if !provides.is_empty() {
            let joined = provides
                .iter()
                .map(|item| format!("\"{item}\""))
                .collect::<Vec<_>>()
                .join(", ");
            manifest.push_str(&format!("provides = [{joined}]\n"));
        }
        manifest.push_str(concat!(
            "[[artifacts]]\n",
            "target = \"x86_64-unknown-linux-gnu\"\n",
            "url = \"https://example.test/artifact.tar.zst\"\n",
            "sha256 = \"abc\"\n"
        ));
        manifest
    }
}
