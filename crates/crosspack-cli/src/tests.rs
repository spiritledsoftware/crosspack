#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;
    use crosspack_registry::RegistrySourceWithSnapshotStatus;
    use ed25519_dalek::{Signer, SigningKey};
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    };

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
            status: TransactionStatus::Failed,
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
            status: TransactionStatus::Failed,
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
            UpgradeCommandOptions {
                dry_run: false,
                explain: false,
                build_from_source: false,
                provider_overrides: &BTreeMap::new(),
                interaction_policy: InstallInteractionPolicy::default(),
            },
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
            status: TransactionStatus::Failed,
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
            status: TransactionStatus::Failed,
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
            status: TransactionStatus::Failed,
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
    fn run_repair_command_recovers_failed_orphan_metadata_transaction() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-orphan-failed";
        let metadata = TransactionMetadata {
            version: 1,
            txid: txid.to_string(),
            operation: "install".to_string(),
            status: TransactionStatus::Failed,
            started_at_unix: 1_771_001_264,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");

        let snapshot_root = layout.transaction_staging_path(txid).join("rollback").join("demo");
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");
        append_transaction_journal_entry(
            &layout,
            txid,
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
            txid,
            &TransactionJournalEntry {
                seq: 2,
                step: "install_package:demo".to_string(),
                state: "done".to_string(),
                path: Some("demo".to_string()),
            },
        )
        .expect("must append mutating step");

        run_repair_command(&layout).expect("repair should recover failed orphan metadata");

        let updated = read_transaction_metadata(&layout, txid)
            .expect("must read updated metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "rolled_back");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_repair_command_recovers_orphan_planning_transaction_with_journal() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-orphan-planning";
        write_transaction_metadata(
            &layout,
            &TransactionMetadata {
                version: 1,
                txid: txid.to_string(),
                operation: "install".to_string(),
                status: TransactionStatus::Planning,
                started_at_unix: 1_771_001_265,
                snapshot_id: None,
            },
        )
        .expect("must write metadata");
        let snapshot_root = layout.transaction_staging_path(txid).join("rollback").join("demo");
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");
        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 1,
                step: "backup_package_state:demo".to_string(),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup-only journal step");

        run_repair_command(&layout).expect("repair should recover orphan planning transaction");

        let updated = read_transaction_metadata(&layout, txid)
            .expect("must read updated metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "rolled_back");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_repair_command_recovers_orphan_rolling_back_transaction() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-orphan-rolling-back";
        write_transaction_metadata(
            &layout,
            &TransactionMetadata {
                version: 1,
                txid: txid.to_string(),
                operation: "install".to_string(),
                status: TransactionStatus::RollingBack,
                started_at_unix: 1_771_001_266,
                snapshot_id: None,
            },
        )
        .expect("must write metadata");
        let snapshot_root = layout.transaction_staging_path(txid).join("rollback").join("demo");
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");
        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 1,
                step: "backup_package_state:demo".to_string(),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup-only journal step");

        run_repair_command(&layout).expect("repair should recover orphan rolling_back transaction");

        let updated = read_transaction_metadata(&layout, txid)
            .expect("must read updated metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "rolled_back");

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
            status: TransactionStatus::Applying,
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
    fn format_repair_action_line_uses_deterministic_action_codes() {
        assert_eq!(
            format_repair_action_line(&TransactionRecoveryAction::Rollback {
                txid: "tx-repair".to_string(),
            }),
            "repair action=rollback"
        );
        assert_eq!(
            format_repair_action_line(&TransactionRecoveryAction::RepairRequired(
                TransactionRepairReason::RollbackEvidenceMissing {
                    txid: "tx-repair".to_string(),
                },
            )),
            "repair action=rollback-evidence-missing"
        );
    }

    #[test]
    fn run_rollback_command_fails_when_journal_replay_required() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-needs-replay".to_string(),
            operation: "install".to_string(),
            status: TransactionStatus::Failed,
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
    fn rollback_restores_backup_only_crash_window_before_apply_done_journal() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-backup-only";
        let package_name = "demo";
        let metadata = TransactionMetadata {
            version: 1,
            txid: txid.to_string(),
            operation: "upgrade".to_string(),
            status: TransactionStatus::Applying,
            started_at_unix: 1_771_001_264,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, txid).expect("must write active marker");

        let old_receipt = install_receipt(package_name, "1.0.0", InstallReason::Root, &[]);
        std::fs::create_dir_all(layout.package_dir(package_name, "1.0.0"))
            .expect("must create old package dir");
        std::fs::write(
            layout.package_dir(package_name, "1.0.0").join("old.txt"),
            "old-state",
        )
        .expect("must write old marker");
        write_install_receipt(&layout, &old_receipt).expect("must write old receipt");

        let snapshot_root =
            capture_package_state_snapshot(&layout, txid, package_name).expect("must capture backup");
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

        std::fs::remove_dir_all(layout.pkgs_dir().join(package_name))
            .expect("must remove old package tree");
        std::fs::create_dir_all(layout.package_dir(package_name, "2.0.0"))
            .expect("must create new package dir");
        std::fs::write(
            layout.package_dir(package_name, "2.0.0").join("new.txt"),
            "new-state",
        )
        .expect("must write new marker");

        run_rollback_command(&layout, Some(txid.to_string()))
            .expect("backup-only crash window must rollback");

        assert!(layout
            .package_dir(package_name, "1.0.0")
            .join("old.txt")
            .exists());
        assert!(!layout.package_dir(package_name, "2.0.0").exists());
        let updated = read_transaction_metadata(&layout, txid)
            .expect("must read updated metadata")
            .expect("metadata must exist");
        assert_eq!(updated.status, "rolled_back");

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
        write_declared_services_state(
            &layout,
            package_name,
            &[ServiceDeclaration {
                name: "demo".to_string(),
                native_id: Some("demo@main".to_string()),
            }],
        )
        .expect("must write declared services sidecar state");

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
        assert!(manifest.declared_services_sidecar_exists);

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
        assert!(
            snapshot_declared_services_sidecar_path(&snapshot_root).exists(),
            "declared services sidecar should be captured"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn capture_snapshot_includes_integration_sidecar_task_8_inventory_gap() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let package_name = "kubectx";
        let package_version = "0.9.5";
        let package_root = layout.package_dir(package_name, package_version);
        std::fs::create_dir_all(&package_root).expect("must create package root");
        std::fs::write(package_root.join("kubectl-ctx"), "#!/bin/sh\n")
            .expect("must write package plugin");

        let projection = IntegrationProjection {
            kind: "path_plugin".to_string(),
            key: "path_plugin:kubectl:ctx".to_string(),
            rel_path: "path-plugins/kubectl/kubectl-ctx".to_string(),
        };
        std::fs::create_dir_all(
            layout
                .integrations_dir()
                .join("path-plugins")
                .join("kubectl"),
        )
        .expect("must create integration dir");
        std::fs::write(layout.integrations_dir().join(&projection.rel_path), "old-plugin")
            .expect("must write integration fixture");
        write_integration_state(&layout, package_name, std::slice::from_ref(&projection))
            .expect("must write integration sidecar");

        let snapshot_root = capture_package_state_snapshot(&layout, "tx-integration", package_name)
            .expect("must capture snapshot");

        std::fs::remove_file(layout.integrations_dir().join(&projection.rel_path))
            .expect("must remove live integration fixture");
        write_integration_state(&layout, package_name, &[])
            .expect("must clear live integration sidecar");
        restore_package_state_snapshot(&layout, package_name, Some(&snapshot_root))
            .expect("must restore integration sidecar snapshot");

        assert_eq!(
            read_integration_state(&layout, package_name).expect("must read restored integrations"),
            vec![projection.clone()],
            "Task 8 inventory gap: integration sidecar rollback payload coverage"
        );
        assert!(
            layout.integrations_dir().join(&projection.rel_path).exists(),
            "integration asset should be restored from rollback payload"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn identity_snapshot_restores_identity_scoped_payload_task_8_inventory_gap() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let identity = InstalledPackageIdentity {
            profile: "default".to_string(),
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            source_namespace: "core".to_string(),
            source_provenance: Some("git:https://example.test/core".to_string()),
            package: "demo".to_string(),
        };
        let receipt = InstallReceipt {
            name: identity.package.clone(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: identity.target.clone(),
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
        };
        let package_root = layout.identity_package_dir(&identity, &receipt.version);
        std::fs::create_dir_all(&package_root).expect("must create identity package root");
        std::fs::write(package_root.join("demo"), "identity-package")
            .expect("must write identity package payload");
        write_identity_install_receipt(&layout, &identity, &receipt)
            .expect("must write identity receipt");
        write_identity_gui_exposure_state(
            &layout,
            &identity,
            &[GuiExposureAsset {
                key: "app:demo".to_string(),
                rel_path: "launchers/demo.desktop".to_string(),
            }],
        )
        .expect("must write identity gui sidecar");
        write_identity_gui_native_state(
            &layout,
            &identity,
            &[GuiNativeRegistrationRecord {
                key: "app:demo".to_string(),
                kind: "desktop-entry".to_string(),
                path: "/tmp/demo.desktop".to_string(),
            }],
        )
        .expect("must write identity native sidecar");
        write_identity_declared_services_state(
            &layout,
            &identity,
            &[ServiceDeclaration {
                name: "demo".to_string(),
                native_id: Some("demo.service".to_string()),
            }],
        )
        .expect("must write identity service sidecar");
        write_identity_integration_state(
            &layout,
            &identity,
            &[IntegrationProjection {
                kind: "path_plugin".to_string(),
                key: "demo".to_string(),
                rel_path: "path/demo/demo".to_string(),
            }],
        )
        .expect("must write identity integration sidecar");
        write_installed_package_state(
            &layout,
            &InstalledPackageState {
                identity: identity.clone(),
                version: receipt.version.clone(),
                receipt: receipt.clone(),
                gui_assets: Vec::new(),
                native_gui_records: Vec::new(),
                services: Vec::new(),
                integrations: Vec::new(),
            },
        )
        .expect("must write identity installed state");

        let snapshot_root = capture_package_state_snapshot(&layout, "tx-identity", &identity.package)
            .expect("must capture identity snapshot");

        std::fs::remove_dir_all(layout.identity_pkgs_dir()).expect("must remove live identity pkgs");
        std::fs::remove_file(layout.identity_receipt_path(&identity))
            .expect("must remove live identity receipt");
        std::fs::remove_file(layout.installed_identity_state_document_path(&identity))
            .expect("must remove live identity state document");
        std::fs::remove_file(layout.identity_gui_state_path(&identity))
            .expect("must remove live identity gui sidecar");
        std::fs::remove_file(layout.identity_gui_native_state_path(&identity))
            .expect("must remove live identity native sidecar");
        std::fs::remove_file(layout.identity_declared_services_state_path(&identity))
            .expect("must remove live identity service sidecar");
        std::fs::remove_file(layout.identity_integration_state_path(&identity))
            .expect("must remove live identity integration sidecar");

        restore_package_state_snapshot(&layout, &identity.package, Some(&snapshot_root))
            .expect("must restore identity snapshot");

        assert!(layout.identity_package_dir(&identity, &receipt.version).join("demo").exists());
        assert!(layout.identity_receipt_path(&identity).exists());
        assert!(layout.installed_identity_state_document_path(&identity).exists());
        assert!(layout.identity_gui_state_path(&identity).exists());
        assert!(layout.identity_gui_native_state_path(&identity).exists());
        assert!(layout.identity_declared_services_state_path(&identity).exists());
        assert!(layout.identity_integration_state_path(&identity).exists());

        let restored_states = read_all_installed_package_states(&layout)
            .expect("must read restored identity state");
        assert_eq!(restored_states.len(), 1);
        assert_eq!(restored_states[0].identity, identity);

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    fn test_installed_identity(package: &str) -> InstalledPackageIdentity {
        InstalledPackageIdentity {
            profile: "default".to_string(),
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            source_namespace: "core".to_string(),
            source_provenance: Some("git:https://example.test/core".to_string()),
            package: package.to_string(),
        }
    }

    #[test]
    fn uninstall_journals_rollback_payload_before_forward_mutation_task_8_inventory_gap() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let receipt = install_receipt("demo", "1.0.0", InstallReason::Root, &[]);
        let identity = InstalledPackageIdentity::from_legacy_receipt(&receipt);
        std::fs::create_dir_all(layout.package_dir("demo", "1.0.0"))
            .expect("must create package dir");
        write_install_receipt(&layout, &receipt).expect("must write receipt");
        write_installed_package_state(
            &layout,
            &InstalledPackageState {
                identity,
                version: receipt.version.clone(),
                receipt: receipt.clone(),
                gui_assets: Vec::new(),
                native_gui_records: Vec::new(),
                services: Vec::new(),
                integrations: Vec::new(),
            },
        )
        .expect("must write installed state");

        run_uninstall_command(&layout, "demo".to_string()).expect("must uninstall demo");
        let txid = single_transaction_txid(&layout);
        let entries = read_transaction_journal_records(&layout, &txid)
            .expect("must read transaction journal");
        let steps = entries
            .iter()
            .map(|entry| entry.step.as_str())
            .collect::<Vec<_>>();

        let backup_index = steps
            .iter()
            .position(|step| *step == "backup_package_state:demo")
            .expect("backup journal entry should exist");
        let uninstall_index = steps
            .iter()
            .position(|step| *step == "uninstall_target:demo")
            .expect("uninstall journal entry should exist");
        assert!(
            backup_index < uninstall_index,
            "Task 8 inventory gap: rollback payload journal entry must precede uninstall mutation journal entry; steps={steps:?}"
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
            status: TransactionStatus::Failed,
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
        let old_declared_services = vec![ServiceDeclaration {
            name: "demo".to_string(),
            native_id: Some("demo@old".to_string()),
        }];
        write_declared_services_state(&layout, package_name, &old_declared_services)
            .expect("must write old declared services sidecar");

        let snapshot_root = layout
            .transaction_staging_path(txid)
            .join("rollback")
            .join(package_name);
        std::fs::create_dir_all(snapshot_root.join("package").join("1.0.0"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipts");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins");
        std::fs::create_dir_all(snapshot_root.join("services"))
            .expect("must create snapshot services dir");
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
        std::fs::copy(
            layout.declared_services_state_path(package_name),
            snapshot_declared_services_sidecar_path(&snapshot_root),
        )
        .expect("must copy declared services fixture into snapshot");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=1\nreceipt_exists=1\nbin=demo\ndeclared_services_sidecar_exists=1\n",
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
        write_declared_services_state(
            &layout,
            package_name,
            &[ServiceDeclaration {
                name: "demo".to_string(),
                native_id: Some("demo@new".to_string()),
            }],
        )
        .expect("must write new declared services sidecar");

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
        let restored_declared_services = read_declared_services_state(&layout, package_name)
            .expect("must read restored declared services sidecar");
        assert_eq!(restored_declared_services, old_declared_services);

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[cfg(unix)]
    #[test]
    fn rollback_replays_integration_enable_created_symlink_removal() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let txid = "tx-integration-enable-rollback";
        write_transaction_metadata(
            &layout,
            &TransactionMetadata {
                version: 1,
                txid: txid.to_string(),
                operation: "integrations".to_string(),
                status: TransactionStatus::Failed,
                started_at_unix: 1_771_001_266,
                snapshot_id: None,
            },
        )
        .expect("must write metadata");
        set_active_transaction(&layout, txid).expect("must write active marker");

        let host_path = layout
            .prefix()
            .join("home")
            .join("user")
            .join(".docker")
            .join("cli-plugins")
            .join("docker-compose");
        std::fs::create_dir_all(host_path.parent().expect("must resolve host parent"))
            .expect("must create host parent");
        let source_path = layout
            .prefix()
            .join("share")
            .join("integrations")
            .join("docker")
            .join("cli-plugins")
            .join("docker-compose");
        std::os::unix::fs::symlink(&source_path, &host_path)
            .expect("must create interrupted activation symlink");
        assert!(
            std::fs::symlink_metadata(&host_path)
                .map(|metadata| metadata.file_type().is_symlink())
                .unwrap_or(false),
            "fixture should create a symlink before rollback"
        );

        let rollback = ActivationRollbackEntry {
            operation: ActivationRollbackOperation::RemoveCreatedSymlink,
            path: host_path.display().to_string(),
            previous_symlink_target: None,
            previous_shim_target: None,
            previous_owner: None,
            created_symlink_target: Some(source_path.display().to_string()),
            created_shim_target: None,
            created_owner: None,
            expected_current_symlink_target: None,
            expected_current_shim_target: None,
            expected_current_owner: None,
            expected_current_absent: false,
            created_parent_dirs: Vec::new(),
        };
        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 1,
                step: "integration_activation_rollback".to_string(),
                state: "planned".to_string(),
                path: Some(serde_json::to_string(&rollback).expect("must serialize rollback")),
            },
        )
        .expect("must append activation rollback payload");

        run_rollback_command(&layout, Some(txid.to_string()))
            .expect("rollback command should replay activation rollback");

        assert!(
            std::fs::symlink_metadata(&host_path).is_err(),
            "rollback should remove symlink created by interrupted activation"
        );
        let updated = read_transaction_metadata(&layout, txid)
            .expect("must read updated metadata")
            .expect("metadata should remain");
        assert_eq!(updated.status, "rolled_back");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[cfg(unix)]
    #[test]
    fn rollback_replays_integration_enable_removes_created_activation_state() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let txid = "tx-integration-enable-state-rollback";
        write_transaction_metadata(
            &layout,
            &TransactionMetadata {
                version: 1,
                txid: txid.to_string(),
                operation: "integrations".to_string(),
                status: TransactionStatus::Failed,
                started_at_unix: 1_771_001_266,
                snapshot_id: None,
            },
        )
        .expect("must write metadata");
        set_active_transaction(&layout, txid).expect("must write active marker");

        let owner = ActivationOwner {
            package_state_key: "default--x86_64-unknown-linux-gnu--core--docker-compose".to_string(),
            package: "docker-compose".to_string(),
            integration_key: "docker_cli_plugin:compose".to_string(),
        };
        let host_path = layout
            .prefix()
            .join("home")
            .join("user")
            .join(".docker")
            .join("cli-plugins")
            .join("docker-compose");
        std::fs::create_dir_all(host_path.parent().expect("must resolve host parent"))
            .expect("must create host parent");
        let source_path = layout
            .prefix()
            .join("share")
            .join("integrations")
            .join("docker")
            .join("cli-plugins")
            .join("docker-compose");
        std::os::unix::fs::symlink(&source_path, &host_path)
            .expect("must create interrupted activation symlink");
        write_integration_activation_state(
            &layout,
            &[IntegrationActivationRecord {
                package_state_key: owner.package_state_key.clone(),
                package: owner.package.clone(),
                integration_key: owner.integration_key.clone(),
                kind: "docker_cli_plugin".to_string(),
                adapter: IntegrationAdapterKind::DockerCli,
                scope: IntegrationActivationScope::None,
                desired_state: IntegrationDesiredState::Enabled,
                applied_state: IntegrationAppliedState::Enabled,
                host_path: Some(host_path.display().to_string()),
                reason_code: IntegrationReasonCode::Ok,
            }],
        )
        .expect("must seed activation record created by failed transaction");

        let rollback = ActivationRollbackEntry {
            operation: ActivationRollbackOperation::RemoveCreatedSymlink,
            path: host_path.display().to_string(),
            previous_symlink_target: None,
            previous_shim_target: None,
            previous_owner: None,
            created_symlink_target: Some(source_path.display().to_string()),
            created_shim_target: None,
            created_owner: Some(owner),
            expected_current_symlink_target: None,
            expected_current_shim_target: None,
            expected_current_owner: None,
            expected_current_absent: false,
            created_parent_dirs: Vec::new(),
        };
        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 1,
                step: "integration_activation_rollback".to_string(),
                state: "planned".to_string(),
                path: Some(serde_json::to_string(&rollback).expect("must serialize rollback")),
            },
        )
        .expect("must append activation rollback payload");

        run_rollback_command(&layout, Some(txid.to_string()))
            .expect("rollback command should replay activation rollback");

        assert!(
            read_integration_activation_state(&layout)
                .expect("must read activation state")
                .is_empty(),
            "rollback should remove activation state created by failed enable"
        );
        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[cfg(unix)]
    #[test]
    fn rollback_replays_integration_disable_owned_symlink_restore() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let txid = "tx-integration-disable-rollback";
        write_transaction_metadata(
            &layout,
            &TransactionMetadata {
                version: 1,
                txid: txid.to_string(),
                operation: "integrations".to_string(),
                status: TransactionStatus::Failed,
                started_at_unix: 1_771_001_266,
                snapshot_id: None,
            },
        )
        .expect("must write metadata");
        set_active_transaction(&layout, txid).expect("must write active marker");

        let host_path = layout.prefix().join("bin").join("democtl");
        let source_path = layout
            .prefix()
            .join("share")
            .join("integrations")
            .join("path")
            .join("demo")
            .join("democtl");
        let owner = ActivationOwner {
            package_state_key: "default--x86_64-unknown-linux-gnu--core--demo".to_string(),
            package: "demo".to_string(),
            integration_key: "path_plugin:demo:democtl".to_string(),
        };
        let rollback = ActivationRollbackEntry {
            operation: ActivationRollbackOperation::RestoreOwnedSymlink,
            path: host_path.display().to_string(),
            previous_symlink_target: Some(source_path.display().to_string()),
            previous_shim_target: None,
            previous_owner: Some(owner),
            created_symlink_target: None,
            created_shim_target: None,
            created_owner: None,
            expected_current_symlink_target: None,
            expected_current_shim_target: None,
            expected_current_owner: None,
            expected_current_absent: true,
            created_parent_dirs: Vec::new(),
        };
        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 1,
                step: "integration_activation_rollback".to_string(),
                state: "planned".to_string(),
                path: Some(serde_json::to_string(&rollback).expect("must serialize rollback")),
            },
        )
        .expect("must append activation rollback payload");

        run_rollback_command(&layout, Some(txid.to_string()))
            .expect("rollback command should replay activation rollback");

        assert_eq!(
            std::fs::read_link(&host_path).expect("must restore owned symlink"),
            source_path
        );
        let updated = read_transaction_metadata(&layout, txid)
            .expect("must read updated metadata")
            .expect("metadata should remain");
        assert_eq!(updated.status, "rolled_back");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn rollback_replays_integration_preview_replacement_payloads() {
        let owner = ActivationOwner {
            package_state_key: "default--host--core--demo".to_string(),
            package: "demo".to_string(),
            integration_key: "path_plugin:demo:tool".to_string(),
        };
        let linux_path_plan = IntegrationActivationPlan {
            package_state_key: owner.package_state_key.clone(),
            package: owner.package.clone(),
            integration_key: owner.integration_key.clone(),
            kind: "path_plugin".to_string(),
            adapter: IntegrationAdapterKind::PathPluginBin,
            scope: IntegrationActivationScope::User,
            desired_state: IntegrationDesiredState::Enabled,
            host_path: "/prefix/bin/tool".to_string(),
            source_path: "/prefix/share/integrations/path/demo/tool-v2".to_string(),
        };
        let mut linux_fs = MemoryActivationFs::new(HostPlatform::Linux);
        linux_fs.write_owned_symlink_for(
            &linux_path_plan.host_path,
            "/prefix/share/integrations/path/demo/tool-v1",
            &owner.package_state_key,
            &owner.package,
            &owner.integration_key,
        );

        let linux_payload = preview_integration_activation_rollback(&linux_fs, &linux_path_plan, true)
            .expect("replacement should journal restore payload");
        assert_eq!(
            linux_payload.operation,
            ActivationRollbackOperation::RestoreOwnedSymlink
        );
        assert_eq!(
            linux_payload.previous_symlink_target.as_deref(),
            Some("/prefix/share/integrations/path/demo/tool-v1")
        );
        assert_eq!(
            linux_payload.expected_current_symlink_target.as_deref(),
            Some("/prefix/share/integrations/path/demo/tool-v2")
        );

        let docker_owner = ActivationOwner {
            package_state_key: "default--host--core--docker-compose".to_string(),
            package: "docker-compose".to_string(),
            integration_key: "docker_cli_plugin:compose".to_string(),
        };
        let docker_plan = IntegrationActivationPlan {
            package_state_key: docker_owner.package_state_key.clone(),
            package: docker_owner.package.clone(),
            integration_key: docker_owner.integration_key.clone(),
            kind: "docker_cli_plugin".to_string(),
            adapter: IntegrationAdapterKind::DockerCli,
            scope: IntegrationActivationScope::None,
            desired_state: IntegrationDesiredState::Enabled,
            host_path: "/home/user/.docker/cli-plugins/docker-compose".to_string(),
            source_path: "/prefix/share/integrations/docker/cli-plugins/docker-compose-v2"
                .to_string(),
        };
        let mut docker_fs = MemoryActivationFs::new(HostPlatform::Linux);
        docker_fs.write_owned_symlink_for(
            &docker_plan.host_path,
            "/prefix/share/integrations/docker/cli-plugins/docker-compose-v1",
            &docker_owner.package_state_key,
            &docker_owner.package,
            &docker_owner.integration_key,
        );

        let docker_payload = preview_integration_activation_rollback(&docker_fs, &docker_plan, true)
            .expect("docker replacement should journal restore payload");
        assert_eq!(
            docker_payload.operation,
            ActivationRollbackOperation::RestoreOwnedSymlink
        );
        assert_eq!(
            docker_payload.previous_symlink_target.as_deref(),
            Some("/prefix/share/integrations/docker/cli-plugins/docker-compose-v1")
        );
        assert_eq!(
            docker_payload.expected_current_symlink_target.as_deref(),
            Some("/prefix/share/integrations/docker/cli-plugins/docker-compose-v2")
        );

        let windows_plan = IntegrationActivationPlan {
            host_path: "C:\\Crosspack\\bin\\tool.cmd".to_string(),
            source_path: "C:\\Crosspack\\share\\integrations\\path\\demo\\tool-v2.exe"
                .to_string(),
            ..linux_path_plan
        };
        let mut windows_fs = MemoryActivationFs::new(HostPlatform::Windows);
        windows_fs.write_owned_shim_for(
            &windows_plan.host_path,
            "C:\\Crosspack\\share\\integrations\\path\\demo\\tool-v1.exe",
            &owner.package_state_key,
            &owner.package,
            &owner.integration_key,
        );

        let windows_payload =
            preview_integration_activation_rollback(&windows_fs, &windows_plan, true)
                .expect("windows shim replacement should journal restore payload");
        assert_eq!(
            windows_payload.operation,
            ActivationRollbackOperation::RestoreOwnedWindowsShim
        );
        assert_eq!(
            windows_payload.previous_shim_target.as_deref(),
            Some("C:\\Crosspack\\share\\integrations\\path\\demo\\tool-v1.exe")
        );
        assert_eq!(
            windows_payload.expected_current_shim_target.as_deref(),
            Some("C:\\Crosspack\\share\\integrations\\path\\demo\\tool-v2.exe")
        );
    }

    #[test]
    fn rollback_restores_activation_state_snapshot_for_uninstall_cleanup() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let package_name = "docker-compose";
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
        .expect("must write receipt");
        let previous = IntegrationActivationRecord {
            package_state_key: "default--x86_64-unknown-linux-gnu--core--docker-compose".to_string(),
            package: package_name.to_string(),
            integration_key: "docker_cli_plugin:compose".to_string(),
            kind: "docker_cli_plugin".to_string(),
            adapter: IntegrationAdapterKind::DockerCli,
            scope: IntegrationActivationScope::None,
            desired_state: IntegrationDesiredState::Enabled,
            applied_state: IntegrationAppliedState::Enabled,
            host_path: Some("/home/user/.docker/cli-plugins/docker-compose".to_string()),
            reason_code: IntegrationReasonCode::Ok,
        };
        let other = IntegrationActivationRecord {
            package_state_key: "default--x86_64-unknown-linux-gnu--core--kubectx".to_string(),
            package: "kubectx".to_string(),
            integration_key: "path_plugin:kubectl:ctx".to_string(),
            kind: "path_plugin".to_string(),
            adapter: IntegrationAdapterKind::PathPluginBin,
            host_path: Some("/prefix/bin/kubectl-ctx".to_string()),
            ..previous.clone()
        };
        write_integration_activation_state(&layout, &[previous.clone(), other.clone()])
            .expect("must seed activation state");

        let snapshot_root = capture_package_state_snapshot(&layout, "tx-activation", package_name)
            .expect("must capture activation state");
        write_integration_activation_state(&layout, std::slice::from_ref(&other))
            .expect("must simulate uninstall activation cleanup");
        restore_package_state_snapshot(&layout, package_name, Some(&snapshot_root))
            .expect("must restore activation state snapshot");

        let records = read_integration_activation_state(&layout).expect("must read activation state");
        assert!(records.contains(&previous));
        assert!(records.contains(&other));
        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn rollback_cleans_declared_services_sidecar_when_snapshot_has_no_sidecar() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-clean-services-sidecar";
        let package_name = "demo";
        write_transaction_metadata(
            &layout,
            &TransactionMetadata {
                version: 1,
                txid: txid.to_string(),
                operation: "upgrade".to_string(),
                status: TransactionStatus::Failed,
                started_at_unix: 1_771_001_307,
                snapshot_id: None,
            },
        )
        .expect("must write metadata");
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
            "package_exists=0\nreceipt_exists=0\ndeclared_services_sidecar_exists=0\n",
        )
        .expect("must write snapshot manifest");

        write_declared_services_state(
            &layout,
            package_name,
            &[ServiceDeclaration {
                name: "demo".to_string(),
                native_id: Some("demo@interrupted".to_string()),
            }],
        )
        .expect("must seed interrupted declared services sidecar");

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
                step: format!("upgrade_package:{package_name}"),
                state: "done".to_string(),
                path: Some(package_name.to_string()),
            },
        )
        .expect("must append mutating step");

        run_rollback_command(&layout, Some(txid.to_string()))
            .expect("rollback should remove declared services sidecar when absent in snapshot");

        assert!(
            !layout.declared_services_state_path(package_name).exists(),
            "declared services sidecar should be removed"
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
            status: TransactionStatus::Failed,
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
    fn rollback_ignores_source_build_journal_steps_and_restores_snapshot_state() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-source-build-rollback";
        let package_name = "demo";
        let metadata = TransactionMetadata {
            version: 1,
            txid: txid.to_string(),
            operation: "install".to_string(),
            status: TransactionStatus::Failed,
            started_at_unix: 1_771_001_300,
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

        std::fs::create_dir_all(layout.pkgs_dir().join(package_name).join("2.0.0"))
            .expect("must create interrupted package state");
        std::fs::write(
            layout
                .pkgs_dir()
                .join(package_name)
                .join("2.0.0")
                .join("partial.txt"),
            "interrupted",
        )
        .expect("must write interrupted package marker");

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
                step: format!("source_fetch:{package_name}"),
                state: "done".to_string(),
                path: Some("/tmp/source-archive.tar.gz".to_string()),
            },
        )
        .expect("must append source fetch step");
        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 3,
                step: format!("source_build_system:{package_name}:cargo"),
                state: "done".to_string(),
                path: None,
            },
        )
        .expect("must append source build system step");
        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 4,
                step: format!("source_install:{package_name}"),
                state: "done".to_string(),
                path: Some(layout.pkgs_dir().join(package_name).display().to_string()),
            },
        )
        .expect("must append source install step");
        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 5,
                step: format!("install_package:{package_name}"),
                state: "done".to_string(),
                path: Some(package_name.to_string()),
            },
        )
        .expect("must append mutating install step");

        run_rollback_command(&layout, Some(txid.to_string()))
            .expect("rollback should succeed with source-build journal steps");

        let updated = read_transaction_metadata(&layout, txid)
            .expect("must read rollback metadata")
            .expect("rollback metadata should exist");
        assert_eq!(updated.status, "rolled_back");
        assert!(
            read_active_transaction(&layout)
                .expect("must read active marker")
                .is_none(),
            "rollback should clear active marker"
        );
        assert!(
            !layout.pkgs_dir().join(package_name).exists(),
            "rollback should remove interrupted source-build package state"
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
            status: TransactionStatus::Failed,
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
        for (status, token) in [
            (TransactionStatus::Planning, "planning"),
            (TransactionStatus::Applying, "applying"),
            (TransactionStatus::RollingBack, "rolling_back"),
            (TransactionStatus::Failed, "failed"),
        ] {
            let layout = test_layout();
            layout.ensure_base_dirs().expect("must create dirs");

            let txid = format!("tx-repair-{}", token.replace('_', "-"));
            let metadata = TransactionMetadata {
                version: 1,
                txid: txid.clone(),
                operation: "install".to_string(),
                status,
                started_at_unix: 1_771_001_267,
                snapshot_id: None,
            };
            write_transaction_metadata(&layout, &metadata).expect("must write metadata");
            set_active_transaction(&layout, &txid).expect("must set active marker");

            let package_name = format!("pkg-{token}");
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
            assert_eq!(updated.status, "rolled_back", "status={token}");
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
            status: TransactionStatus::Applying,
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
            status: TransactionStatus::Failed,
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
            status: TransactionStatus::Failed,
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
            status: TransactionStatus::Failed,
            started_at_unix: 1_771_001_100,
            snapshot_id: None,
        };
        let newer = TransactionMetadata {
            version: 1,
            txid: "tx-new-failed".to_string(),
            operation: "upgrade".to_string(),
            status: TransactionStatus::Failed,
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
    fn run_rollback_command_rejects_empty_active_marker() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        std::fs::write(layout.transaction_active_path(), "\n")
            .expect("must write empty active marker fixture");

        let err = run_rollback_command(&layout, None)
            .expect_err("rollback must fail closed on empty active marker");
        assert!(
            err.to_string().contains("reason=active_marker_invalid"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_rejects_corrupt_active_marker_for_explicit_txid() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let txid = "tx-explicit-corrupt-active";
        let metadata = TransactionMetadata {
            version: 1,
            txid: txid.to_string(),
            operation: "install".to_string(),
            status: TransactionStatus::Failed,
            started_at_unix: 1_771_001_250,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        std::fs::write(layout.transaction_active_path(), "../escape\n")
            .expect("must write corrupt active marker fixture");

        let err = run_rollback_command(&layout, Some(txid.to_string()))
            .expect_err("explicit rollback must fail closed on corrupt active marker");
        assert!(
            err.to_string().contains("reason=active_marker_invalid"),
            "unexpected error: {err}"
        );

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
            status: TransactionStatus::Applying,
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
            status: TransactionStatus::Applying,
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
            status: TransactionStatus::Failed,
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
            status: TransactionStatus::Failed,
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
            status: TransactionStatus::Failed,
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
            status: TransactionStatus::RollingBack,
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
            status: TransactionStatus::Failed,
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
            status: TransactionStatus::Planning,
            started_at_unix: 1_771_001_300,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-abc").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("active transaction must include status context");
        assert!(
            err.to_string()
                .contains("transaction tx-abc is active (reason=active_status status=planning)"),
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
            status: TransactionStatus::Committed,
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
    fn ensure_no_active_transaction_fails_closed_on_metadata_txid_mismatch() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-other".to_string(),
            operation: "install".to_string(),
            status: TransactionStatus::Committed,
            started_at_unix: 1_771_001_361,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        std::fs::rename(
            layout.transaction_metadata_path("tx-other"),
            layout.transaction_metadata_path("tx-marker"),
        )
        .expect("must move mismatched metadata into marker path");
        set_active_transaction(&layout, "tx-marker").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("metadata txid mismatch must require repair");
        assert!(
            err.to_string().contains(
                "transaction state requires repair tx-marker (reason=metadata_txid_mismatch expected=tx-marker actual=tx-other)"
            ),
            "unexpected error: {err}"
        );
        assert_eq!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .as_deref(),
            Some("tx-marker"),
            "mismatched committed metadata must not clear active marker"
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
            status: TransactionStatus::Planning,
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
            status: TransactionStatus::RolledBack,
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

        set_transaction_status(&layout, &tx.txid, TransactionStatus::Applying).expect("must update status");

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
            set_transaction_status(&layout, &tx.txid, TransactionStatus::RollingBack)?;
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
            set_transaction_status(&layout, &tx.txid, TransactionStatus::RolledBack)?;
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
            set_transaction_status(&layout, &tx.txid, TransactionStatus::RolledBack)?;
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
            set_transaction_status(&layout, &tx.txid, TransactionStatus::Committed)?;
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
            status: TransactionStatus::Applying,
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
            status: TransactionStatus::RollingBack,
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
            status: TransactionStatus::Failed,
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
            status: TransactionStatus::RollingBack,
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
            status: TransactionStatus::Applying,
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
            status: TransactionStatus::Applying,
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
            status: TransactionStatus::Committed,
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
    fn doctor_transaction_detail_line_reports_active_metadata_and_latest_step() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-detail".to_string(),
            operation: "install".to_string(),
            status: TransactionStatus::Applying,
            started_at_unix: 1_771_001_662,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-detail").expect("must write active marker");
        append_transaction_journal_entry(
            &layout,
            "tx-detail",
            &TransactionJournalEntry {
                seq: 1,
                step: "resolve_plan:demo".to_string(),
                state: "done".to_string(),
                path: Some("demo".to_string()),
            },
        )
        .expect("must append first journal entry");
        append_transaction_journal_entry(
            &layout,
            "tx-detail",
            &TransactionJournalEntry {
                seq: 2,
                step: "install_package:demo".to_string(),
                state: "done".to_string(),
                path: Some("demo".to_string()),
            },
        )
        .expect("must append latest journal entry");

        let line = doctor_transaction_detail_line(&layout)
            .expect("detail line should render")
            .expect("active trusted metadata should produce detail line");
        assert_eq!(
            line,
            "transaction_detail txid=tx-detail status=applying operation=install step=install_package:demo"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_detail_line_is_best_effort_for_broken_state() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        set_active_transaction(&layout, "tx-broken-detail").expect("must write active marker");
        std::fs::write(
            layout.transaction_metadata_path("tx-broken-detail"),
            "not metadata",
        )
        .expect("must write corrupt metadata");

        assert_eq!(
            doctor_transaction_detail_line(&layout).expect("broken detail should not fail doctor"),
            None
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_fails_closed_on_metadata_txid_mismatch() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-other".to_string(),
            operation: "install".to_string(),
            status: TransactionStatus::Committed,
            started_at_unix: 1_771_001_661,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        std::fs::rename(
            layout.transaction_metadata_path("tx-other"),
            layout.transaction_metadata_path("tx-marker"),
        )
        .expect("must move mismatched metadata into marker path");
        set_active_transaction(&layout, "tx-marker").expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should report mismatched metadata");
        assert_eq!(
            line,
            "transaction: failed tx-marker (reason=metadata_txid_mismatch expected=tx-marker actual=tx-other)"
        );
        assert_eq!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .as_deref(),
            Some("tx-marker"),
            "mismatched committed metadata must not clear active marker"
        );

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
            status: TransactionStatus::Planning,
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
            status: TransactionStatus::Committed,
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
            source_build: None,
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
    fn install_plan_application_skips_missing_replacement_receipts() {
        let plan = crosspack_resolver::InstallPlan {
            operation: crosspack_resolver::PlanOperation::Upgrade,
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            packages: vec![crosspack_resolver::PlannedPackage {
                name: "ripgrep".to_string(),
                version: "14.1.1".to_string(),
                target: "x86_64-unknown-linux-gnu".to_string(),
                install_reason: "root".to_string(),
                dependencies: Vec::new(),
            }],
            removals: Vec::new(),
            replacements: vec![crosspack_resolver::PlannedReplacement {
                removed_name: "ripgrep-legacy".to_string(),
                removed_version: "13.0.0".to_string(),
                replacement_name: "ripgrep".to_string(),
                replacement_version: "14.1.1".to_string(),
                requirement: "<14.0.0".to_string(),
            }],
            transitions: Vec::new(),
            provider_substitutions: Vec::new(),
            conflicts: Vec::new(),
            risk_flags: Vec::new(),
        };

        let application = install_plan_application_for_package(&plan, "ripgrep", &[], &[])
            .expect("missing replacement receipt should not abort plan application");

        assert!(application.replacement_receipts.is_empty());
        assert_eq!(application.install_reason, InstallReason::Root);
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
            ArchiveType::TarXz,
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
    fn cli_parses_install_with_dry_run_explain_flag() {
        let cli =
            Cli::try_parse_from(["crosspack", "install", "ripgrep", "--dry-run", "--explain"])
                .expect("command must parse");

        match cli.command {
            Commands::Install {
                dry_run, explain, ..
            } => {
                assert!(dry_run);
                assert!(explain);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn install_explain_without_dry_run_is_rejected() {
        let cli = Cli::try_parse_from(["crosspack", "install", "ripgrep", "--explain"])
            .expect("command must parse");
        let err = run_cli(cli).expect_err("--explain must require --dry-run");
        assert_eq!(
            err.to_string(),
            "--explain requires --dry-run for 'install'"
        );
    }

    #[test]
    fn cli_parses_install_with_build_from_source_flag() {
        let cli = Cli::try_parse_from(["crosspack", "install", "ripgrep", "--build-from-source"])
            .expect("command must parse");

        match cli.command {
            Commands::Install {
                build_from_source, ..
            } => {
                assert!(build_from_source);
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
    fn cli_parses_upgrade_with_dry_run_explain_flag() {
        let cli =
            Cli::try_parse_from(["crosspack", "upgrade", "ripgrep", "--dry-run", "--explain"])
                .expect("command must parse");

        match cli.command {
            Commands::Upgrade {
                dry_run, explain, ..
            } => {
                assert!(dry_run);
                assert!(explain);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_upgrade_with_build_from_source_flag() {
        let cli = Cli::try_parse_from(["crosspack", "upgrade", "ripgrep", "--build-from-source"])
            .expect("command must parse");

        match cli.command {
            Commands::Upgrade {
                build_from_source, ..
            } => {
                assert!(build_from_source);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_bundle_export_with_optional_output_flag() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "bundle",
            "export",
            "--output",
            "state/export.toml",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::Bundle {
                command: BundleCommands::Export { output },
            } => {
                assert_eq!(output, Some(PathBuf::from("state/export.toml")));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_bundle_apply_with_flags() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "bundle",
            "apply",
            "--file",
            "state/bundle.toml",
            "--dry-run",
            "--force-redownload",
            "--provider",
            "c-compiler=clang",
            "--provider",
            "rust-toolchain=rustup",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::Bundle {
                command:
                    BundleCommands::Apply {
                        file,
                        dry_run,
                        force_redownload,
                        provider,
                        ..
                    },
            } => {
                assert_eq!(file, Some(PathBuf::from("state/bundle.toml")));
                assert!(dry_run);
                assert!(force_redownload);
                assert_eq!(provider, vec!["c-compiler=clang", "rust-toolchain=rustup"]);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_bundle_apply_with_dry_run_explain_flag() {
        let cli = Cli::try_parse_from(["crosspack", "bundle", "apply", "--dry-run", "--explain"])
            .expect("command must parse");

        match cli.command {
            Commands::Bundle {
                command:
                    BundleCommands::Apply {
                        dry_run, explain, ..
                    },
            } => {
                assert!(dry_run);
                assert!(explain);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_bundle_apply_with_build_from_source_flag() {
        let cli = Cli::try_parse_from(["crosspack", "bundle", "apply", "--build-from-source"])
            .expect("command must parse");

        match cli.command {
            Commands::Bundle {
                command:
                    BundleCommands::Apply {
                        build_from_source, ..
                    },
            } => {
                assert!(build_from_source);
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
            Commands::Uninstall {
                name, escalation, ..
            } => {
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
    fn cli_parses_outdated_subcommand() {
        let cli = Cli::try_parse_from(["crosspack", "outdated"]).expect("command must parse");
        assert!(matches!(cli.command, Commands::Outdated));
    }

    #[test]
    fn cli_parses_depends_subcommand() {
        let cli =
            Cli::try_parse_from(["crosspack", "depends", "ripgrep"]).expect("command must parse");
        match cli.command {
            Commands::Depends { name } => assert_eq!(name, "ripgrep"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_uses_subcommand() {
        let cli = Cli::try_parse_from(["crosspack", "uses", "pcre2"]).expect("command must parse");
        match cli.command {
            Commands::Uses { name } => assert_eq!(name, "pcre2"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_why_subcommand() {
        let cli = Cli::try_parse_from(["crosspack", "why", "pcre2"]).expect("command parses");
        match cli.command {
            Commands::Why { name } => assert_eq!(name, "pcre2"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_cache_subcommands() {
        let list = Cli::try_parse_from(["crosspack", "cache", "list"]).expect("list parses");
        match list.command {
            Commands::Cache {
                command: CacheCommands::List,
            } => {}
            other => panic!("unexpected command: {other:?}"),
        }

        let prune = Cli::try_parse_from(["crosspack", "cache", "prune"]).expect("prune parses");
        match prune.command {
            Commands::Cache {
                command: CacheCommands::Prune,
            } => {}
            other => panic!("unexpected command: {other:?}"),
        }

        let gc = Cli::try_parse_from(["crosspack", "cache", "gc"]).expect("gc parses");
        match gc.command {
            Commands::Cache {
                command: CacheCommands::Gc,
            } => {}
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_services_subcommands() {
        let list = Cli::try_parse_from(["crosspack", "services", "list"]).expect("list parses");
        match list.command {
            Commands::Services {
                command: ServicesCommands::List,
            } => {}
            other => panic!("unexpected command: {other:?}"),
        }

        let status = Cli::try_parse_from(["crosspack", "services", "status", "demo", "web"])
            .expect("status parses");
        match status.command {
            Commands::Services {
                command: ServicesCommands::Status { package, service },
            } => {
                assert_eq!(package, "demo");
                assert_eq!(service, "web");
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let start = Cli::try_parse_from(["crosspack", "services", "start", "demo", "web"])
            .expect("start parses");
        match start.command {
            Commands::Services {
                command: ServicesCommands::Start { package, service },
            } => {
                assert_eq!(package, "demo");
                assert_eq!(service, "web");
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let stop = Cli::try_parse_from(["crosspack", "services", "stop", "demo", "web"])
            .expect("stop parses");
        match stop.command {
            Commands::Services {
                command: ServicesCommands::Stop { package, service },
            } => {
                assert_eq!(package, "demo");
                assert_eq!(service, "web");
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let restart = Cli::try_parse_from(["crosspack", "services", "restart", "demo", "web"])
            .expect("restart parses");
        match restart.command {
            Commands::Services {
                command: ServicesCommands::Restart { package, service },
            } => {
                assert_eq!(package, "demo");
                assert_eq!(service, "web");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_integrations_enable_disable_subcommands() {
        let enable = Cli::try_parse_from([
            "crosspack",
            "integrations",
            "enable",
            "docker-compose",
            "compose",
        ])
        .expect("enable parses");
        match enable.command {
            Commands::Integrations {
                command: IntegrationsCommands::Enable {
                    package,
                    integration,
                },
            } => {
                assert_eq!(package, "docker-compose");
                assert_eq!(integration, "compose");
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let disable = Cli::try_parse_from([
            "crosspack",
            "integrations",
            "disable",
            "docker-compose",
            "docker_cli_plugin:compose",
        ])
        .expect("disable parses");
        match disable.command {
            Commands::Integrations {
                command: IntegrationsCommands::Disable {
                    package,
                    integration,
                },
            } => {
                assert_eq!(package, "docker-compose");
                assert_eq!(integration, "docker_cli_plugin:compose");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn find_dependency_path_from_roots_returns_shortest_root_path() {
        let root_a = InstallReceipt {
            name: "root-a".to_string(),
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
        };
        let shared = InstallReceipt {
            name: "shared".to_string(),
            version: "1.0.0".to_string(),
            dependencies: vec!["leaf@1.0.0".to_string()],
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
        };
        let leaf = InstallReceipt {
            name: "leaf".to_string(),
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
        };

        let receipt_map = HashMap::from([
            (root_a.name.clone(), &root_a),
            (shared.name.clone(), &shared),
            (leaf.name.clone(), &leaf),
        ]);
        let roots = vec!["root-a".to_string()];

        let path = find_dependency_path_from_roots("leaf", &roots, &receipt_map)
            .expect("dependency path should exist");
        assert_eq!(path, vec!["root-a", "shared", "leaf"]);
    }

    #[test]
    fn safe_artifact_cache_path_rejects_parent_traversal() {
        let layout = test_layout();
        let invalid = format!("{}/../escape.bin", layout.artifacts_cache_dir().display());
        assert_eq!(safe_artifact_cache_path(&layout, &invalid), None);
    }

    #[test]
    fn safe_artifact_cache_path_accepts_absolute_artifacts_path() {
        let layout = test_layout();
        let valid = layout
            .artifacts_cache_dir()
            .join("ripgrep/14.1.0/x86_64-unknown-linux-gnu/artifact.tar.zst");
        let resolved = safe_artifact_cache_path(&layout, &valid.display().to_string())
            .expect("path should be accepted");
        assert_eq!(resolved, valid);
    }

    #[test]
    fn managed_service_state_transitions_are_deterministic_when_native_not_applied() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "demo".to_string(),
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
        .expect("must write receipt");
        write_declared_services_state(
            &layout,
            "demo",
            &[crosspack_core::ServiceDeclaration {
                name: "demo".to_string(),
                native_id: Some("crosspack-missing-service-for-tests".to_string()),
            }],
        )
        .expect("must write declared services state");

        assert_eq!(
            read_managed_service_state(&layout, "demo").expect("must read default state"),
            ManagedServiceState::Stopped
        );

        run_service_start_command(&layout, "demo").expect("start must succeed");
        assert_eq!(
            read_managed_service_state(&layout, "demo")
                .expect("must preserve stopped state when native action fails"),
            ManagedServiceState::Stopped
        );
        assert!(
            !managed_service_state_path(&layout, "demo").exists(),
            "state file should not be created when native start is not applied"
        );

        run_service_stop_command(&layout, "demo").expect("stop must succeed");
        assert_eq!(
            read_managed_service_state(&layout, "demo").expect("must read stopped state"),
            ManagedServiceState::Stopped
        );
        assert!(
            !managed_service_state_path(&layout, "demo").exists(),
            "state file should remain absent when native stop is not applied"
        );

        run_service_restart_command(&layout, "demo").expect("restart must succeed");
        assert_eq!(
            read_managed_service_state(&layout, "demo")
                .expect("must preserve stopped state on restart failure"),
            ManagedServiceState::Stopped
        );
        assert!(
            !managed_service_state_path(&layout, "demo").exists(),
            "state file should remain absent when native restart is not applied"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn service_output_contract_lines_include_adapter_applied_and_reason_keys() {
        let outcome = NativeServiceOutcome {
            adapter: "systemd".to_string(),
            applied: false,
            reason_code: "adapter-tool-missing".to_string(),
        };

        assert_eq!(
            render_service_state_line("demo", ManagedServiceState::Stopped, None, &outcome),
            "service_state name=demo state=stopped adapter=systemd applied=false reason=adapter-tool-missing"
        );
        assert_eq!(
            render_service_state_line(
                "demo",
                ManagedServiceState::Stopped,
                Some("start"),
                &outcome
            ),
            "service_state name=demo state=stopped action=start adapter=systemd applied=false reason=adapter-tool-missing"
        );
        assert_eq!(
            render_service_state_line(
                "demo",
                ManagedServiceState::Stopped,
                Some("stop"),
                &outcome
            ),
            "service_state name=demo state=stopped action=stop adapter=systemd applied=false reason=adapter-tool-missing"
        );
        assert_eq!(
            render_service_state_line(
                "demo",
                ManagedServiceState::Stopped,
                Some("restart"),
                &outcome
            ),
            "service_state name=demo state=stopped action=restart adapter=systemd applied=false reason=adapter-tool-missing"
        );
    }

    #[test]
    fn service_commands_require_declared_service_presence() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "missing".to_string(),
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
        .expect("must write receipt");

        let err = run_service_start_command(&layout, "missing")
            .expect_err("service start should require declared service metadata");
        let message = err.to_string();
        assert!(message.contains("No declared service found: missing"));
        assert!(message.contains("crosspack install"));

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn managed_services_list_rows_are_sorted_and_filtered_to_installed_packages() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        for name in ["bravo", "alpha", "charlie"] {
            write_install_receipt(
                &layout,
                &InstallReceipt {
                    name: name.to_string(),
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
            .expect("must write receipt");
        }
        write_declared_services_state(
            &layout,
            "alpha",
            &[crosspack_core::ServiceDeclaration {
                name: "alpha".to_string(),
                native_id: None,
            }],
        )
        .expect("must write declared services");
        write_declared_services_state(
            &layout,
            "charlie",
            &[crosspack_core::ServiceDeclaration {
                name: "charlie".to_string(),
                native_id: Some("charlie-daemon".to_string()),
            }],
        )
        .expect("must write declared services");

        write_managed_service_state(&layout, "charlie", ManagedServiceState::Running)
            .expect("must write running service state");
        write_managed_service_state(&layout, "alpha", ManagedServiceState::Stopped)
            .expect("must write stopped service state");
        write_managed_service_state(&layout, "ghost", ManagedServiceState::Running)
            .expect("must write non-installed service state");

        let rows =
            collect_managed_service_rows(&layout).expect("must collect managed service rows");
        let rendered = rows
            .iter()
            .map(|row| format!("{} {}", row.name, row.state.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(rendered, vec!["alpha stopped", "charlie running"]);

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn managed_services_list_reports_applied_false_for_ok_but_stopped_activation() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let receipt = InstallReceipt {
            name: "caddy".to_string(),
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
        };
        let package_state_key = InstalledPackageIdentity::from_legacy_receipt(&receipt).state_key();
        write_install_receipt(&layout, &receipt)
        .expect("must write receipt");
        write_declared_services_state(
            &layout,
            "caddy",
            &[crosspack_core::ServiceDeclaration {
                name: "caddy".to_string(),
                native_id: None,
            }],
        )
        .expect("must write declared services");
        write_integration_activation_state(
            &layout,
            &[
                IntegrationActivationRecord {
                    package_state_key: "stale--x86_64-unknown-linux-gnu--core--caddy".to_string(),
                    package: "caddy".to_string(),
                    integration_key: "service:caddy".to_string(),
                    kind: "service".to_string(),
                    adapter: IntegrationAdapterKind::SystemdUser,
                    scope: IntegrationActivationScope::User,
                    desired_state: IntegrationDesiredState::Running,
                    applied_state: IntegrationAppliedState::Running,
                    host_path: Some("systemd-user:caddy.service".to_string()),
                    reason_code: IntegrationReasonCode::Ok,
                },
                IntegrationActivationRecord {
                    package_state_key,
                    package: "caddy".to_string(),
                    integration_key: "service:caddy".to_string(),
                    kind: "service".to_string(),
                    adapter: IntegrationAdapterKind::SystemdUser,
                    scope: IntegrationActivationScope::User,
                    desired_state: IntegrationDesiredState::Projected,
                    applied_state: IntegrationAppliedState::Stopped,
                    host_path: Some("systemd-user:caddy.service".to_string()),
                    reason_code: IntegrationReasonCode::Ok,
                },
            ],
        )
        .expect("must seed activation state");

        let rows = collect_managed_service_rows(&layout).expect("must collect service rows");
        assert_eq!(format_managed_service_row(&rows[0]), "service package=caddy name=caddy state=stopped adapter=systemd-user scope=user applied=false reason=ok");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn service_commands_accept_plus_in_package_name() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "cpp+tool".to_string(),
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
        .expect("must write receipt");
        write_declared_services_state(
            &layout,
            "cpp+tool",
            &[crosspack_core::ServiceDeclaration {
                name: "cpp+tool".to_string(),
                native_id: None,
            }],
        )
        .expect("must write declared services state");

        run_service_start_command(&layout, "cpp+tool").expect("start must succeed");
        assert_eq!(
            read_managed_service_state(&layout, "cpp+tool")
                .expect("must keep stopped state when start is not applied"),
            ManagedServiceState::Stopped
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn read_managed_service_state_rejects_duplicate_state_entries() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let path = managed_service_state_path(&layout, "demo");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("must create service state dir");
        }
        std::fs::write(&path, "state=running\nstate=stopped\n")
            .expect("must write duplicate service state file");

        let err = read_managed_service_state(&layout, "demo")
            .expect_err("duplicate state lines should fail");
        assert!(err.to_string().contains("duplicate service state entries"));

        let _ = std::fs::remove_dir_all(layout.prefix());
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
    fn lifecycle_boundary_types_construct_and_render_lines() {
        let install = InstallCommandRequest {
            spec: "ripgrep@^14".to_string(),
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            dry_run: true,
            force_redownload: false,
            build_from_source: false,
            explain: true,
            provider_overrides: vec!["compiler=clang".to_string()],
        };
        assert_eq!(install.spec, "ripgrep@^14");
        assert!(install.dry_run);

        let upgrade = UpgradeCommandRequest {
            spec: Some("ripgrep@^14".to_string()),
            target: None,
            dry_run: true,
            force_redownload: true,
            build_from_source: false,
            explain: false,
            provider_overrides: Vec::new(),
        };
        assert_eq!(upgrade.spec.as_deref(), Some("ripgrep@^14"));
        assert!(upgrade.force_redownload);

        let uninstall = UninstallCommandRequest {
            name: "ripgrep".to_string(),
        };
        assert_eq!(uninstall.name, "ripgrep");

        let lines = render_lifecycle_outcome(LifecycleCommandOutcome::Lines(vec![
            "ok".to_string(),
        ]));
        assert_eq!(lines, vec!["ok".to_string()]);
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
    fn install_plan_preview_lines_match_existing_transaction_preview_output() {
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
        let plan = install_plan_from_transaction_preview(
            PlanOperation::Upgrade,
            Some("x86_64-unknown-linux-gnu".to_string()),
            &preview,
        );

        assert_eq!(
            render_install_plan_preview_lines(&plan, TransactionPreviewMode::DryRun, None),
            render_dry_run_output_lines(&preview, TransactionPreviewMode::DryRun, None)
        );
    }

    #[test]
    fn install_transaction_preview_dry_run_output_matches_lifecycle_contract() {
        let resolved = vec![resolved_install("tool", "1.2.3")];
        let planned = build_planned_package_changes(&resolved, &[])
            .expect("install dry-run planned changes must build");
        let preview = build_transaction_preview("install", &planned);

        let lines = render_dry_run_output_lines(&preview, TransactionPreviewMode::DryRun, None);

        assert_eq!(
            lines,
            vec![
                "transaction_preview operation=install mode=dry-run".to_string(),
                "transaction_summary adds=1 removals=0 replacements=0 transitions=0".to_string(),
                "risk_flags=adds".to_string(),
                "change_add name=tool version=1.2.3 target=x86_64-unknown-linux-gnu".to_string(),
            ]
        );
    }

    #[test]
    fn upgrade_named_transaction_preview_dry_run_output_matches_lifecycle_contract() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "tool"
version = "2.0.0"

[replaces]
old-tool = "<1.0.0"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/tool-2.0.0.tar.zst"
sha256 = "abc"
"#,
        )
        .expect("manifest should parse");
        let resolved = vec![ResolvedInstall {
            artifact: manifest.artifacts[0].clone(),
            manifest,
            resolved_target: "x86_64-unknown-linux-gnu".to_string(),
            archive_type: ArchiveType::TarZst,
            source_build: None,
        }];
        let receipts = vec![
            install_receipt("tool", "1.0.0", InstallReason::Root, &[]),
            install_receipt("old-tool", "0.9.0", InstallReason::Root, &[]),
        ];
        let planned = build_planned_package_changes(&resolved, &receipts)
            .expect("upgrade dry-run planned changes must build");
        let preview = build_transaction_preview("upgrade", &planned);

        let lines = render_dry_run_output_lines(&preview, TransactionPreviewMode::DryRun, None);

        assert_eq!(
            lines,
            vec![
                "transaction_preview operation=upgrade mode=dry-run".to_string(),
                "transaction_summary adds=0 removals=1 replacements=1 transitions=1".to_string(),
                "risk_flags=removals,replacements,version-transitions".to_string(),
                "change_remove name=old-tool version=0.9.0 reason=replacement".to_string(),
                "change_replace from=old-tool@0.9.0 to=tool@2.0.0".to_string(),
                "change_transition name=tool from=1.0.0 to=2.0.0".to_string(),
            ]
        );
    }

    #[test]
    fn upgrade_all_transaction_preview_dry_run_output_matches_lifecycle_contract() {
        let resolved = vec![resolved_install("app", "2.0.0"), resolved_install("dep", "1.1.0")];
        let receipts = vec![install_receipt(
            "app",
            "1.0.0",
            InstallReason::Root,
            &["dep@1.0.0"],
        )];
        let planned = build_planned_package_changes(&resolved, &receipts)
            .expect("upgrade-all dry-run planned changes must build");
        let preview = build_transaction_preview("upgrade", &planned);

        let lines = render_dry_run_output_lines(&preview, TransactionPreviewMode::DryRun, None);

        assert_eq!(
            lines,
            vec![
                "transaction_preview operation=upgrade mode=dry-run".to_string(),
                "transaction_summary adds=1 removals=0 replacements=0 transitions=1".to_string(),
                "risk_flags=adds,multi-package-transaction,version-transitions".to_string(),
                "change_add name=dep version=1.1.0 target=x86_64-unknown-linux-gnu".to_string(),
                "change_transition name=app from=1.0.0 to=2.0.0".to_string(),
            ]
        );
    }

    #[test]
    fn uninstall_blocked_by_dependents_transaction_preview_contract_matches_lifecycle_output() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_install_receipt(
            &layout,
            &install_receipt("app-a", "1.0.0", InstallReason::Root, &["shared@1.0.0"]),
        )
        .expect("must seed first root receipt");
        write_install_receipt(
            &layout,
            &install_receipt("app-b", "1.0.0", InstallReason::Root, &["shared@1.0.0"]),
        )
        .expect("must seed second root receipt");
        write_install_receipt(
            &layout,
            &install_receipt("shared", "1.0.0", InstallReason::Dependency, &[]),
        )
        .expect("must seed shared dependency receipt");

        let result = uninstall_package(&layout, "shared").expect("uninstall should be blocked");
        let lines = format_uninstall_messages(&result);

        assert_eq!(
            lines,
            vec!["cannot uninstall shared 1.0.0: still required by roots app-a, app-b".to_string()]
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
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
    fn dry_run_output_without_explain_matches_existing_contract_lines() {
        let preview = build_transaction_preview(
            "upgrade",
            &[PlannedPackageChange {
                name: "tool".to_string(),
                target: "x86_64-unknown-linux-gnu".to_string(),
                new_version: "2.0.0".to_string(),
                old_version: Some("1.0.0".to_string()),
                replacement_removals: Vec::new(),
            }],
        );

        let contract_lines =
            render_transaction_preview_lines(&preview, TransactionPreviewMode::DryRun);
        let without_explain =
            render_dry_run_output_lines(&preview, TransactionPreviewMode::DryRun, None);

        assert_eq!(without_explain, contract_lines);
    }

    #[test]
    fn explainability_lines_are_deterministic_for_provider_replacement_and_conflicts() {
        let tool_manifest = PackageManifest::from_toml_str(
            r#"
name = "tool"
version = "1.0.0"

[dependencies]
c-compiler = "*"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/tool-1.0.0.tar.zst"
sha256 = "abc"
"#,
        )
        .expect("manifest should parse");
        let provider_manifest = PackageManifest::from_toml_str(
            r#"
name = "clang"
version = "18.0.0"
provides = ["c-compiler"]

[conflicts]
gcc = "*"
legacy-cc = "<2.0.0"

[replaces]
old-cc = "<2.0.0"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/clang-18.0.0.tar.zst"
sha256 = "abc"
"#,
        )
        .expect("manifest should parse");

        let resolved = vec![
            ResolvedInstall {
                artifact: provider_manifest.artifacts[0].clone(),
                manifest: provider_manifest,
                resolved_target: "x86_64-unknown-linux-gnu".to_string(),
                archive_type: ArchiveType::TarZst,
                source_build: None,
            },
            ResolvedInstall {
                artifact: tool_manifest.artifacts[0].clone(),
                manifest: tool_manifest,
                resolved_target: "x86_64-unknown-linux-gnu".to_string(),
                archive_type: ArchiveType::TarZst,
                source_build: None,
            },
        ];
        let receipts = vec![InstallReceipt {
            name: "old-cc".to_string(),
            version: "1.5.0".to_string(),
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
        }];
        let roots = vec![RootInstallRequest {
            name: "tool".to_string(),
            requirement: VersionReq::STAR,
        }];

        let explainability = build_dependency_policy_explainability(&resolved, &receipts, &roots)
            .expect("must build explainability");
        let lines = render_dependency_policy_explainability_lines(&explainability);

        assert_eq!(
            lines,
            vec![
                "explain_provider capability=c-compiler selected=clang@18.0.0".to_string(),
                "explain_replacement selected=clang@18.0.0 removes=old-cc@1.5.0 declared=<2.0.0"
                    .to_string(),
                "explain_conflict selected=clang@18.0.0 conflicts=gcc(*)".to_string(),
                "explain_conflict selected=clang@18.0.0 conflicts=legacy-cc(<2.0.0)".to_string(),
            ]
        );
    }

    #[test]
    fn explainability_includes_multiple_provider_substitutions_for_same_capability() {
        let app_manifest = PackageManifest::from_toml_str(
            r#"
name = "app"
version = "1.0.0"

[dependencies]
c-compiler = "*"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/app-1.0.0.tar.zst"
sha256 = "abc"
"#,
        )
        .expect("manifest should parse");
        let clang_manifest = PackageManifest::from_toml_str(
            r#"
name = "clang"
version = "18.0.0"
provides = ["c-compiler"]

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/clang-18.0.0.tar.zst"
sha256 = "abc"
"#,
        )
        .expect("manifest should parse");
        let zigcc_manifest = PackageManifest::from_toml_str(
            r#"
name = "zigcc"
version = "0.12.0"
provides = ["c-compiler"]

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/zigcc-0.12.0.tar.zst"
sha256 = "abc"
"#,
        )
        .expect("manifest should parse");

        let resolved = vec![
            ResolvedInstall {
                artifact: app_manifest.artifacts[0].clone(),
                manifest: app_manifest,
                resolved_target: "x86_64-unknown-linux-gnu".to_string(),
                archive_type: ArchiveType::TarZst,
                source_build: None,
            },
            ResolvedInstall {
                artifact: zigcc_manifest.artifacts[0].clone(),
                manifest: zigcc_manifest,
                resolved_target: "x86_64-unknown-linux-gnu".to_string(),
                archive_type: ArchiveType::TarZst,
                source_build: None,
            },
            ResolvedInstall {
                artifact: clang_manifest.artifacts[0].clone(),
                manifest: clang_manifest,
                resolved_target: "x86_64-unknown-linux-gnu".to_string(),
                archive_type: ArchiveType::TarZst,
                source_build: None,
            },
        ];

        let roots = vec![RootInstallRequest {
            name: "app".to_string(),
            requirement: VersionReq::STAR,
        }];
        let explainability = build_dependency_policy_explainability(&resolved, &[], &roots)
            .expect("must build explainability");
        let lines = render_dependency_policy_explainability_lines(&explainability);
        assert!(lines
            .contains(&"explain_provider capability=c-compiler selected=clang@18.0.0".to_string()));
        assert!(lines
            .contains(&"explain_provider capability=c-compiler selected=zigcc@0.12.0".to_string()));
    }

    #[test]
    fn explain_requires_dry_run_error_is_actionable() {
        let err = ensure_explain_requires_dry_run("install", false, true)
            .expect_err("--explain without --dry-run should fail");
        assert_eq!(
            err.to_string(),
            "--explain requires --dry-run for 'install'"
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
    fn bundle_export_document_orders_roots_and_pins_deterministically() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "zeta".to_string(),
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
                installed_at_unix: 2,
            },
        )
        .expect("must write zeta receipt");
        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "alpha".to_string(),
                version: "3.0.0".to_string(),
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
                installed_at_unix: 3,
            },
        )
        .expect("must write alpha receipt");
        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "dep".to_string(),
                version: "2.5.0".to_string(),
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
        )
        .expect("must write dependency receipt");
        write_pin(&layout, "zeta", "^1").expect("must write zeta pin");

        let bundle = build_export_bundle_document(&layout).expect("must build bundle");
        assert_eq!(bundle.roots.len(), 2);
        assert_eq!(bundle.roots[0].name, "alpha");
        assert_eq!(
            bundle.roots[0].target.as_deref(),
            Some("aarch64-apple-darwin")
        );
        assert_eq!(bundle.roots[0].requirement.as_deref(), Some("=3.0.0"));
        assert_eq!(bundle.roots[1].name, "zeta");
        assert_eq!(
            bundle.roots[1].target.as_deref(),
            Some("x86_64-unknown-linux-gnu")
        );
        assert_eq!(bundle.roots[1].requirement.as_deref(), Some("^1"));

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn parse_bundle_document_rejects_unknown_fields() {
        let raw = r#"
format = "crosspack.bundle"
version = 1
unexpected = "value"

[[roots]]
name = "ripgrep"
requirement = "^14"
"#;

        let err = parse_bundle_document(raw).expect_err("unknown fields should be rejected");
        let rendered = err
            .chain()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            rendered.contains("unknown field") && rendered.contains("unexpected"),
            "unexpected parse error: {rendered}"
        );
    }

    #[test]
    fn bundle_apply_group_plans_reject_cross_target_overlap() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        configure_ready_source(&layout, "official");
        let primary_target = host_target_triple();
        let secondary_target = if primary_target == "x86_64-unknown-linux-gnu" {
            "aarch64-apple-darwin"
        } else {
            "x86_64-unknown-linux-gnu"
        };
        write_signed_test_manifest_with_targets(
            &layout,
            TestManifestSpec {
                source_name: "official",
                package_name: "ripgrep",
                version: "14.1.0",
                license: None,
                homepage: None,
                provides: &[],
                targets: &[primary_target, secondary_target],
            },
        );

        let backend = select_metadata_backend(None, &layout).expect("backend must load");
        let bundle = BundleDocument {
            format: BUNDLE_FORMAT_MARKER.to_string(),
            version: BUNDLE_FORMAT_VERSION,
            roots: vec![
                BundleRoot {
                    name: "ripgrep".to_string(),
                    target: Some(primary_target.to_string()),
                    requirement: Some("^14".to_string()),
                },
                BundleRoot {
                    name: "ripgrep".to_string(),
                    target: Some(secondary_target.to_string()),
                    requirement: Some("^14".to_string()),
                },
            ],
            snapshot_context: None,
        };

        let err =
            build_bundle_apply_group_plans(&layout, &backend, &bundle, &BTreeMap::new(), false)
                .expect_err("overlap across target groups must fail");
        assert!(
            err.to_string()
                .contains("cannot safely process package 'ripgrep'"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn bundle_apply_dry_run_preview_includes_stable_transaction_keys() {
        let preview = build_transaction_preview(
            "bundle-apply",
            &[PlannedPackageChange {
                name: "tool".to_string(),
                target: "x86_64-unknown-linux-gnu".to_string(),
                new_version: "1.2.3".to_string(),
                old_version: None,
                replacement_removals: Vec::new(),
            }],
        );

        let first = render_transaction_preview_lines(&preview, TransactionPreviewMode::DryRun);
        let second = render_transaction_preview_lines(&preview, TransactionPreviewMode::DryRun);

        assert_eq!(first, second);
        assert!(first
            .iter()
            .any(|line| line.starts_with("transaction_preview ")));
        assert!(first
            .iter()
            .any(|line| line.starts_with("transaction_summary ")));
        assert!(first.iter().any(|line| line.starts_with("risk_flags=")));
        assert!(first.iter().any(|line| line.starts_with("change_add ")));
    }

    #[test]
    fn bundle_apply_with_missing_file_returns_actionable_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let missing = layout.prefix().join("missing-bundle.toml");
        let err = load_bundle_document_from_path(&missing)
            .expect_err("missing file should return actionable error");
        let rendered = err.to_string();
        assert!(rendered.contains("bundle file not found"));
        assert!(rendered.contains(missing.to_string_lossy().as_ref()));
        assert!(rendered.contains("--file <path>"));

        let _ = std::fs::remove_dir_all(layout.prefix());
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
    fn init_shell_snippet_loads_package_shell_init_after_path_and_completions() {
        let layout = PrefixLayout::new(build_test_layout_path(current_unix_nanos()).join("with spaces"));
        layout.ensure_base_dirs().expect("must create dirs");

        for shell in [
            CliCompletionShell::Bash,
            CliCompletionShell::Zsh,
            CliCompletionShell::Fish,
            CliCompletionShell::Powershell,
        ] {
            let rendered = init_shell_snippet(&layout, shell);
            let completion_marker = layout
                .completions_dir()
                .display()
                .to_string();
            let shell_init_marker = layout
                .shell_init_shell_dir(shell.package_completion_shell())
                .display()
                .to_string();
            assert!(rendered.contains(&completion_marker));
            assert!(rendered.contains(&shell_init_marker));
            assert!(
                rendered.find(&completion_marker) < rendered.find(&shell_init_marker),
                "shell init loader should follow completion loader for {shell:?}"
            );
            assert!(
                rendered.contains("sort") || rendered.contains("Sort-Object FullName"),
                "shell init loader should be deterministic for {shell:?}"
            );
        }

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn init_shell_snippet_exposes_manpath_for_unix_shells_only() {
        let layout = PrefixLayout::new(build_test_layout_path(current_unix_nanos()).join("with spaces"));
        layout.ensure_base_dirs().expect("must create dirs");

        for shell in [
            CliCompletionShell::Bash,
            CliCompletionShell::Zsh,
            CliCompletionShell::Fish,
        ] {
            let rendered = init_shell_snippet(&layout, shell);
            let man_marker = layout.man_dir().display().to_string();
            assert!(rendered.contains(&man_marker));
            assert!(rendered.contains("MANPATH"));
        }

        let powershell = init_shell_snippet(&layout, CliCompletionShell::Powershell);
        assert!(!powershell.contains("MANPATH"));

        let _ = std::fs::remove_dir_all(layout.prefix());
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
    fn generate_completions_uses_crosspack_and_cpk_command_names() {
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
            assert!(
                rendered.contains("cpk"),
                "completion script should target short alias binary name for {shell:?}"
            );
        }
        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn powershell_completion_script_is_safe_to_dot_source_from_profiles() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut output = Vec::new();
        write_completions_script(CliCompletionShell::Powershell, &layout, &mut output)
            .expect("completion script generation should succeed");
        let rendered = String::from_utf8(output).expect("completion script should be utf-8");

        assert!(
            !rendered.contains("using namespace"),
            "PowerShell completion script must not require using statements to appear first"
        );
        assert!(rendered.contains("[System.Management.Automation.CompletionResult]::new"));
        assert!(
            rendered.contains(
                "[System.Management.Automation.Language.StringConstantExpressionAst]"
            )
        );
        assert!(rendered.contains("[System.Management.Automation.Language.StringConstantType]"));

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
        let cpk_compdef_index = rendered
            .find("compdef _cpk cpk")
            .expect("zsh script should register cpk completion function");

        assert!(
            compinit_index < compdef_index,
            "zsh script must initialize completion system before compdef registration"
        );
        assert!(
            compinit_index < cpk_compdef_index,
            "zsh script must initialize completion system before cpk compdef registration"
        );
    }

    #[test]
    fn zsh_completion_script_initializes_compinit_without_package_completion_dir() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        assert!(
            !layout
                .package_completions_shell_dir(ArtifactCompletionShell::Zsh)
                .exists(),
            "fresh test prefix should not have a zsh package completion dir"
        );

        let mut output = Vec::new();
        write_completions_script(CliCompletionShell::Zsh, &layout, &mut output)
            .expect("completion script generation should succeed");
        let rendered = String::from_utf8(output).expect("completion script should be utf-8");

        let compinit_index = rendered
            .find("compinit -i")
            .expect("zsh script should initialize compinit");
        let package_dir_check_index = rendered
            .find("if [ -d '")
            .expect("zsh script should conditionally register package completion dir");
        let compdef_index = rendered
            .find("compdef _crosspack crosspack")
            .expect("zsh script should register crosspack completion function");

        assert!(
            compinit_index < package_dir_check_index,
            "zsh script must initialize completion system even without package completions"
        );
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
    fn apply_provider_override_rejects_non_provider_package() {
        let tool = PackageManifest::from_toml_str(
            r#"
name = "tool"
version = "1.0.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/tool-1.0.0.tar.zst"
sha256 = "tool"
"#,
        )
        .expect("tool manifest must parse");

        let mut overrides = BTreeMap::new();
        overrides.insert("compiler".to_string(), "tool".to_string());

        let err = apply_provider_override("compiler", vec![tool], &overrides)
            .expect_err("non-provider package must be rejected distinctly");
        assert!(
            err.to_string()
                .contains("package 'tool' does not provide capability 'compiler'"),
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
    fn configured_registry_resolves_capability_provider_packages() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        configure_ready_source(&layout, "official");
        let target = host_target_triple();
        write_signed_policy_manifest(
            &layout,
            "official",
            "app",
            &format!(
                r#"
name = "app"
version = "1.0.0"
[dependencies]
compiler = "*"
[[artifacts]]
target = "{target}"
url = "https://example.test/app-1.0.0.tar.zst"
sha256 = "app"
"#
            ),
        );
        write_signed_policy_manifest(
            &layout,
            "official",
            "gcc",
            &format!(
                r#"
name = "gcc"
version = "1.5.0"
provides = ["compiler"]
[[artifacts]]
target = "{target}"
url = "https://example.test/gcc-1.5.0.tar.zst"
sha256 = "gcc"
"#
            ),
        );
        write_signed_policy_manifest(
            &layout,
            "official",
            "llvm",
            &format!(
                r#"
name = "llvm"
version = "2.0.0"
provides = ["compiler"]
[[artifacts]]
target = "{target}"
url = "https://example.test/llvm-2.0.0.tar.zst"
sha256 = "llvm"
"#
            ),
        );
        write_signed_policy_manifest(
            &layout,
            "official",
            "broken-unrelated",
            &format!(
                r#"
name = "broken-unrelated"
version = "9.9.9"
[[artifacts]]
target = "{target}"
url = "https://example.test/broken-unrelated-9.9.9.tar.zst"
sha256 = "broken"
"#
            ),
        );

        let backend = select_metadata_backend(None, &layout).expect("backend must load");
        let roots = vec![RootInstallRequest {
            name: "app".to_string(),
            requirement: VersionReq::STAR,
        }];
        let resolved = resolve_install_graph(
            &layout,
            &backend,
            &roots,
            Some(target),
            &BTreeMap::new(),
            false,
        )
        .expect("configured registry should resolve provider package");

        let names = resolved
            .iter()
            .map(|package| package.manifest.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"app"));
        assert!(names.contains(&"llvm"));

        let plan = build_install_plan_from_resolved(
            PlanOperation::Install,
            Some(target.to_string()),
            &resolved,
            &[],
            &roots,
        );
        let explainability = dependency_policy_explainability_from_install_plan(&plan);
        assert_eq!(
            render_dependency_policy_explainability_lines(&explainability),
            vec!["explain_provider capability=compiler selected=llvm@2.0.0".to_string()]
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn installed_manifest_lookup_preserves_same_name_versions() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        configure_ready_source(&layout, "official");
        let target = host_target_triple();
        write_signed_policy_manifest(
            &layout,
            "official",
            "gcc",
            &format!(
                r#"
name = "gcc"
version = "0.9.0"
provides = ["compiler"]
[[artifacts]]
target = "{target}"
url = "https://example.test/gcc-0.9.0.tar.zst"
sha256 = "gcc09"
"#
            ),
        );
        write_signed_policy_manifest(
            &layout,
            "official",
            "gcc",
            &format!(
                r#"
name = "gcc"
version = "1.5.0"
provides = ["compiler"]
[[artifacts]]
target = "{target}"
url = "https://example.test/gcc-1.5.0.tar.zst"
sha256 = "gcc15"
"#
            ),
        );
        let backend = select_metadata_backend(None, &layout).expect("backend must load");
        let receipts = vec![
            install_receipt("gcc", "0.9.0", InstallReason::Dependency, &[]),
            install_receipt("gcc", "1.5.0", InstallReason::Dependency, &[]),
        ];

        let installed = installed_manifests_for_receipts(&backend, &receipts)
            .expect("installed manifest lookup must preserve duplicates");
        assert_eq!(installed.len(), 2);
        assert!(installed.iter().any(|manifest| manifest.version.to_string() == "0.9.0"));
        assert!(installed.iter().any(|manifest| manifest.version.to_string() == "1.5.0"));

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn configured_registry_provider_candidate_signature_failure_fails_closed() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        configure_ready_source(&layout, "official");
        let target = host_target_triple();
        write_signed_policy_manifest(
            &layout,
            "official",
            "app",
            &format!(
                r#"
name = "app"
version = "1.0.0"
[dependencies]
compiler = "*"
[[artifacts]]
target = "{target}"
url = "https://example.test/app-1.0.0.tar.zst"
sha256 = "app"
"#
            ),
        );
        write_signed_policy_manifest(
            &layout,
            "official",
            "llvm",
            &format!(
                r#"
name = "llvm"
version = "2.0.0"
provides = ["compiler"]
[[artifacts]]
target = "{target}"
url = "https://example.test/llvm-2.0.0.tar.zst"
sha256 = "llvm"
"#
            ),
        );
        write_invalid_policy_manifest(
            &layout,
            "official",
            "tampered-cc",
            &format!(
                r#"
name = "tampered-cc"
version = "9.9.9"
provides = ["compiler"]
[[artifacts]]
target = "{target}"
url = "https://example.test/tampered-cc-9.9.9.tar.zst"
sha256 = "tampered"
"#
            ),
        );

        let backend = select_metadata_backend(None, &layout).expect("backend must load");
        let roots = vec![RootInstallRequest {
            name: "app".to_string(),
            requirement: VersionReq::STAR,
        }];
        let err = resolve_install_graph(
            &layout,
            &backend,
            &roots,
            Some(target),
            &BTreeMap::new(),
            false,
        )
        .expect_err("invalid provider candidate metadata must fail closed");
        assert!(
            err.to_string().contains("failed loading provider metadata")
                || err.to_string().contains("signature verification failed"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn configured_registry_upgrade_prefers_installed_provider_package() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        configure_ready_source(&layout, "official");
        let target = host_target_triple();
        write_signed_policy_manifest(
            &layout,
            "official",
            "app",
            &format!(
                r#"
name = "app"
version = "1.0.0"
[dependencies]
compiler = ">=1.0.0, <3.0.0"
[[artifacts]]
target = "{target}"
url = "https://example.test/app-1.0.0.tar.zst"
sha256 = "app"
"#
            ),
        );
        write_signed_policy_manifest(
            &layout,
            "official",
            "gcc",
            &format!(
                r#"
name = "gcc"
version = "1.5.0"
provides = ["compiler"]
[[artifacts]]
target = "{target}"
url = "https://example.test/gcc-1.5.0.tar.zst"
sha256 = "gcc"
"#
            ),
        );
        write_signed_policy_manifest(
            &layout,
            "official",
            "llvm",
            &format!(
                r#"
name = "llvm"
version = "2.0.0"
provides = ["compiler"]
[[artifacts]]
target = "{target}"
url = "https://example.test/llvm-2.0.0.tar.zst"
sha256 = "llvm"
"#
            ),
        );
        write_install_receipt(
            &layout,
            &install_receipt("gcc", "1.5.0", InstallReason::Dependency, &[]),
        )
        .expect("must seed installed provider receipt");

        let backend = select_metadata_backend(None, &layout).expect("backend must load");
        let roots = vec![RootInstallRequest {
            name: "app".to_string(),
            requirement: VersionReq::STAR,
        }];
        let resolved = resolve_install_graph(
            &layout,
            &backend,
            &roots,
            Some(target),
            &BTreeMap::new(),
            false,
        )
        .expect("configured registry should keep valid installed provider");

        assert!(resolved.iter().any(|package| package.manifest.name == "gcc"));
        assert!(!resolved.iter().any(|package| package.manifest.name == "llvm"));

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn configured_registry_policy_fixture_rejects_conflicting_graph() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        configure_ready_source(&layout, "official");
        let target = host_target_triple();
        write_signed_policy_manifest(
            &layout,
            "official",
            "app",
            &format!(
                r#"
name = "app"
version = "1.0.0"
[dependencies]
foo = "*"
bar = "*"
[[artifacts]]
target = "{target}"
url = "https://example.test/app-1.0.0.tar.zst"
sha256 = "app"
"#
            ),
        );
        write_signed_policy_manifest(
            &layout,
            "official",
            "foo",
            &format!(
                r#"
name = "foo"
version = "1.0.0"
[conflicts]
bar = "*"
[[artifacts]]
target = "{target}"
url = "https://example.test/foo-1.0.0.tar.zst"
sha256 = "foo"
"#
            ),
        );
        write_signed_policy_manifest(
            &layout,
            "official",
            "bar",
            &format!(
                r#"
name = "bar"
version = "1.0.0"
[[artifacts]]
target = "{target}"
url = "https://example.test/bar-1.0.0.tar.zst"
sha256 = "bar"
"#
            ),
        );

        let backend = select_metadata_backend(None, &layout).expect("backend must load");
        let roots = vec![RootInstallRequest {
            name: "app".to_string(),
            requirement: VersionReq::STAR,
        }];
        let err = resolve_install_graph(
            &layout,
            &backend,
            &roots,
            Some(target),
            &BTreeMap::new(),
            false,
        )
        .expect_err("conflicting configured registry graph must fail");
        assert!(
            err.to_string()
                .contains("no compatible dependency graph found"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn configured_registry_policy_fixture_renders_replacement_dry_run_evidence() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        configure_ready_source(&layout, "official");
        let target = host_target_triple();
        write_signed_policy_manifest(
            &layout,
            "official",
            "clang",
            &format!(
                r#"
name = "clang"
version = "18.0.0"
[replaces]
old-cc = "<2.0.0"
[[artifacts]]
target = "{target}"
url = "https://example.test/clang-18.0.0.tar.zst"
sha256 = "clang"
"#
            ),
        );
        write_signed_policy_manifest(
            &layout,
            "official",
            "old-cc",
            &format!(
                r#"
name = "old-cc"
version = "1.5.0"
[[artifacts]]
target = "{target}"
url = "https://example.test/old-cc-1.5.0.tar.zst"
sha256 = "old-cc"
"#
            ),
        );
        let old_receipt = install_receipt("old-cc", "1.5.0", InstallReason::Root, &[]);
        write_install_receipt(&layout, &old_receipt).expect("must seed replaced receipt");

        let backend = select_metadata_backend(None, &layout).expect("backend must load");
        let roots = vec![RootInstallRequest {
            name: "clang".to_string(),
            requirement: VersionReq::STAR,
        }];
        let resolved = resolve_install_graph(
            &layout,
            &backend,
            &roots,
            Some(target),
            &BTreeMap::new(),
            false,
        )
        .expect("replacement fixture should resolve");
        let plan = build_install_plan_from_resolved(
            PlanOperation::Install,
            Some(target.to_string()),
            &resolved,
            &[old_receipt],
            &roots,
        );

        assert_eq!(plan.replacements.len(), 1);
        assert_eq!(plan.replacements[0].removed_name, "old-cc");
        let explainability = dependency_policy_explainability_from_install_plan(&plan);
        let lines = render_install_plan_preview_lines(
            &plan,
            TransactionPreviewMode::DryRun,
            Some(&explainability),
        );
        assert!(
            lines.iter().any(|line| line
                == "change_replace from=old-cc@1.5.0 to=clang@18.0.0"),
            "replacement dry-run line missing: {lines:?}"
        );
        assert!(
            lines.iter().any(|line| line
                == "explain_replacement selected=clang@18.0.0 removes=old-cc@1.5.0 declared=<2.0.0"),
            "replacement explain line missing: {lines:?}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn format_info_lines_includes_sanitized_description_when_present() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "compiler"
version = "2.1.0"
 description = "Fast\tline\nsearch\rtool"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/compiler.tar.zst"
sha256 = "abc"
"#,
        )
        .expect("manifest must parse");

        let lines = format_info_lines("compiler", &[manifest]);
        assert_eq!(lines[0], "Package: compiler");
        assert_eq!(lines[1], "- 2.1.0");
        assert_eq!(lines[2], "  Description: Fast line search tool");
    }

    #[test]
    fn format_info_lines_preserves_policy_order_with_and_without_description() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "compiler"
version = "2.1.0"
description = "Portable toolchain"
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
        assert_eq!(lines[2], "  Description: Portable toolchain");
        assert_eq!(lines[3], "  Provides: c-compiler, cc");
        assert_eq!(lines[4], "  Conflicts: legacy-cc(*)");
        assert_eq!(lines[5], "  Replaces: old-cc(<2.0.0)");
        assert_eq!(lines[6], "  Policy: provides=2 conflicts=1 replaces=1");

        let manifest_without_description = PackageManifest::from_toml_str(
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

        let lines_without_description =
            format_info_lines("compiler", &[manifest_without_description]);
        assert_eq!(lines_without_description[0], "Package: compiler");
        assert_eq!(lines_without_description[1], "- 2.1.0");
        assert_eq!(lines_without_description[2], "  Provides: c-compiler, cc");
        assert_eq!(lines_without_description[3], "  Conflicts: legacy-cc(*)");
        assert_eq!(lines_without_description[4], "  Replaces: old-cc(<2.0.0)");
        assert_eq!(
            lines_without_description[5],
            "  Policy: provides=2 conflicts=1 replaces=1"
        );
    }

    #[test]
    fn format_info_lines_omits_description_when_only_whitespace() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "compiler"
version = "2.1.0"
description = "   \n\t"
"#,
        )
        .expect("manifest must parse");

        let lines = format_info_lines("compiler", &[manifest]);
        assert_eq!(lines[0], "Package: compiler");
        assert_eq!(lines[1], "- 2.1.0");
        assert!(
            !lines.iter().any(|line| line.starts_with("  Description:")),
            "whitespace-only descriptions must not be rendered"
        );
    }

    #[test]
    fn format_info_lines_for_style_plain_preserves_contract() {
        let manifest = PackageManifest {
            name: "ripgrep".to_string(),
            version: Version::parse("14.1.0").unwrap(),
            description: Some("line search".to_string()),
            license: Some("MIT".to_string()),
            homepage: Some("https://github.com/BurntSushi/ripgrep".to_string()),
            provides: Vec::new(),
            conflicts: BTreeMap::new(),
            replaces: BTreeMap::new(),
            dependencies: BTreeMap::new(),
            artifacts: Vec::new(),
            source_build: None,
            services: Vec::new(),
            integrations: Vec::new(),
            shell_init: Vec::new(),
        };

        assert_eq!(
            format_info_lines_for_style(
                OutputStyle::Plain,
                "ripgrep",
                std::slice::from_ref(&manifest),
            ),
            format_info_lines("ripgrep", &[manifest])
        );
    }

    #[test]
    fn format_info_lines_for_style_rich_adds_sectioned_details() {
        let manifest = PackageManifest {
            name: "ripgrep".to_string(),
            version: Version::parse("14.1.0").unwrap(),
            description: Some("line search".to_string()),
            license: Some("MIT".to_string()),
            homepage: Some("https://github.com/BurntSushi/ripgrep".to_string()),
            provides: Vec::new(),
            conflicts: BTreeMap::new(),
            replaces: BTreeMap::new(),
            dependencies: BTreeMap::new(),
            artifacts: Vec::new(),
            source_build: None,
            services: Vec::new(),
            integrations: Vec::new(),
            shell_init: Vec::new(),
        };

        let lines = format_info_lines_for_style(OutputStyle::Rich, "ripgrep", &[manifest]);

        assert!(lines.contains(&"✓ ripgrep".to_string()));
        assert!(lines.contains(&"  version    14.1.0".to_string()));
        assert!(lines.contains(&"  summary    line search".to_string()));
        assert!(lines.contains(&"  license    MIT".to_string()));
    }

    #[test]
    fn format_info_lines_for_style_rich_preserves_policy_details() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "compiler"
version = "2.1.0"
description = "Portable toolchain"
provides = ["c-compiler", "cc"]

[conflicts]
legacy-cc = "*"

[replaces]
old-cc = "<2.0.0"
"#,
        )
        .expect("manifest must parse");

        let lines = format_info_lines_for_style(OutputStyle::Rich, "compiler", &[manifest]);

        assert!(lines.contains(&"  provides   c-compiler, cc".to_string()));
        assert!(lines.contains(&"  conflicts  legacy-cc(*)".to_string()));
        assert!(lines.contains(&"  replaces   old-cc(<2.0.0)".to_string()));
        assert!(lines.contains(&"  policy     provides=2 conflicts=1 replaces=1".to_string()));
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
                    community: None,
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
                    community: None,
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
    fn format_registry_list_status_lines_rich_adds_badges_without_changing_plain_lines() {
        let sources = vec![RegistrySourceWithSnapshotState {
            source: RegistrySourceRecord {
                name: "core".to_string(),
                kind: RegistrySourceKind::Git,
                location: "https://github.com/spiritledsoftware/crosspack-registry.git".to_string(),
                fingerprint_sha256: "abc123".to_string(),
                enabled: true,
                priority: 100,
                community: None,
            },
            snapshot: RegistrySourceSnapshotState::Ready {
                snapshot_id: "snap-1".to_string(),
            },
        }];

        let plain = format_registry_list_status_lines(OutputStyle::Plain, sources.clone());
        assert_eq!(plain, format_registry_list_lines(sources.clone()));

        let rich = format_registry_list_status_lines(OutputStyle::Rich, sources);
        assert!(rich.iter().any(|line| line.starts_with("✓")));
        assert!(rich.iter().any(|line| line.contains("snapshot")));
    }

    #[test]
    fn format_registry_list_status_lines_rich_warns_for_none_snapshot_from_state() {
        let sources = vec![RegistrySourceWithSnapshotState {
            source: RegistrySourceRecord {
                name: "mirror".to_string(),
                kind: RegistrySourceKind::Git,
                location: "https://example.test/registry?snapshot=ready:misleading".to_string(),
                fingerprint_sha256: "abc123".to_string(),
                enabled: true,
                priority: 100,
                community: None,
            },
            snapshot: RegistrySourceSnapshotState::None,
        }];

        let rich = format_registry_list_status_lines(OutputStyle::Rich, sources);
        assert_eq!(
            rich,
            vec!["! mirror kind=git priority=100 location=https://example.test/registry?snapshot=ready:misleading snapshot=none".to_string()]
        );
    }

    #[test]
    fn format_registry_list_status_lines_rich_warns_for_error_snapshot_from_state() {
        let sources = vec![RegistrySourceWithSnapshotState {
            source: RegistrySourceRecord {
                name: "mirror".to_string(),
                kind: RegistrySourceKind::Git,
                location: "https://example.test/registry?snapshot=ready:misleading".to_string(),
                fingerprint_sha256: "abc123".to_string(),
                enabled: true,
                priority: 100,
                community: None,
            },
            snapshot: RegistrySourceSnapshotState::Error {
                status: RegistrySourceWithSnapshotStatus::Unreadable,
                reason_code: "snapshot-unreadable".to_string(),
            },
        }];

        let rich = format_registry_list_status_lines(OutputStyle::Rich, sources);
        assert_eq!(
            rich,
            vec!["! mirror kind=git priority=100 location=https://example.test/registry?snapshot=ready:misleading snapshot=error:snapshot-unreadable".to_string()]
        );
    }

    #[test]
    fn format_installed_list_lines_for_style_rich_empty_includes_hint() {
        assert_eq!(
            format_installed_list_lines_for_style(OutputStyle::Rich, &[]),
            vec![
                "! No installed packages".to_string(),
                "• Run `crosspack install <name>` to install a package.".to_string(),
            ]
        );
    }

    #[test]
    fn format_installed_list_lines_plain_preserves_receipt_order_and_plain_output() {
        let receipts = vec![
            install_receipt("zeta", "2.0.0", InstallReason::Root, &[]),
            install_receipt("alpha", "1.0.0", InstallReason::Root, &[]),
        ];

        assert_eq!(
            format_installed_list_lines_for_style(OutputStyle::Plain, &receipts),
            vec!["zeta 2.0.0".to_string(), "alpha 1.0.0".to_string()]
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
                community: None,
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
                community: None,
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
                community: None,
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
                community: None,
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
                community: None,
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
                community: None,
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
                community: None,
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
                community: None,
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
                community: None,
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
        std::fs::create_dir_all(state_root.join("cache/official/releases/ripgrep"))
            .expect("must create source cache structure");
        std::fs::create_dir_all(state_root.join("cache/official/packages"))
            .expect("must create package template cache structure");
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
        let outcome = run_search_command(&backend, "rip").expect("search must succeed");
        let lines = format_search_results(&outcome.results, "rip");

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
    fn run_search_command_returns_skipped_package_warnings() {
        let layout = test_layout();
        configure_ready_source(&layout, "official");
        write_signed_test_manifest(
            &layout,
            "official",
            "good",
            "1.0.0",
            Some("MIT"),
            None,
            &[],
        );

        let cache_root = registry_state_root(&layout).join("cache").join("official");
        let signing_key = test_signing_key();
        let package_template_path = cache_root.join("packages").join("bad.toml");
        let bad_template = "name = [\"bad\"\n";
        std::fs::write(&package_template_path, bad_template.as_bytes())
            .expect("must write malformed package template");
        let package_signature = signing_key.sign(bad_template.as_bytes());
        std::fs::write(
            package_template_path.with_extension("toml.sig"),
            hex::encode(package_signature.to_bytes()),
        )
        .expect("must write malformed package signature");
        let bad_dir = cache_root.join("releases").join("bad");
        std::fs::create_dir_all(&bad_dir).expect("must create bad release dir");
        let release = "version = \"1.0.0\"\n";
        let release_path = bad_dir.join("1.0.0.toml");
        std::fs::write(&release_path, release.as_bytes()).expect("must write bad release");
        let release_signature = signing_key.sign(release.as_bytes());
        std::fs::write(
            release_path.with_extension("toml.sig"),
            hex::encode(release_signature.to_bytes()),
        )
        .expect("must write bad release signature");

        let backend = select_metadata_backend(None, &layout).expect("configured backend must load");
        let outcome = run_search_command(&backend, "").expect("search must succeed");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].name, "good");
        assert_eq!(outcome.skipped_packages.len(), 1);
        assert_eq!(outcome.skipped_packages[0].package, "bad");
        assert_eq!(
            outcome.skipped_packages[0].reason_code,
            "package-metadata-invalid"
        );
        assert_eq!(
            format_package_skip_warning(&outcome.skipped_packages[0]),
            format!(
                "warning: registry_package_skipped package=\"bad\" reason=\"package-metadata-invalid\" source=\"official\" detail={:?}",
                format!(
                    "failed parsing package template: {}",
                    package_template_path.display()
                )
            )
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_search_command_uses_lower_priority_valid_package_when_preferred_source_is_poisoned() {
        let layout = test_layout();
        let state_root = registry_state_root(&layout);
        std::fs::create_dir_all(&state_root).expect("must create registry state root");
        std::fs::write(
            state_root.join("sources.toml"),
            "version = 1\n\n[[sources]]\nname = \"preferred\"\nkind = \"filesystem\"\nlocation = \"/tmp/preferred\"\nfingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\nenabled = true\npriority = 0\n\n[[sources]]\nname = \"fallback\"\nkind = \"filesystem\"\nlocation = \"/tmp/fallback\"\nfingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\nenabled = true\npriority = 1\n",
        )
        .expect("must write sources state");
        configure_ready_cache_source(&layout, "preferred");
        configure_ready_cache_source(&layout, "fallback");

        write_signed_test_manifest(
            &layout,
            "fallback",
            "ripgrep",
            "14.1.0",
            Some("MIT"),
            None,
            &[],
        );

        let preferred_root = state_root.join("cache").join("preferred");
        let signing_key = test_signing_key();
        std::fs::write(preferred_root.join("registry.pub"), public_key_hex(&signing_key))
            .expect("must write preferred registry key");
        let package_template_path = preferred_root.join("packages").join("ripgrep.toml");
        std::fs::create_dir_all(preferred_root.join("packages"))
            .expect("must create preferred packages dir");
        let bad_template = "name = [\"ripgrep\"\n";
        std::fs::write(&package_template_path, bad_template.as_bytes())
            .expect("must write malformed preferred template");
        let package_signature = signing_key.sign(bad_template.as_bytes());
        std::fs::write(
            package_template_path.with_extension("toml.sig"),
            hex::encode(package_signature.to_bytes()),
        )
        .expect("must write preferred package signature");
        let release_dir = preferred_root.join("releases").join("ripgrep");
        let release = "version = \"99.0.0\"\n";
        let release_path = release_dir.join("99.0.0.toml");
        std::fs::create_dir_all(&release_dir).expect("must create preferred release dir");
        std::fs::write(&release_path, release.as_bytes())
            .expect("must write preferred release");
        let release_signature = signing_key.sign(release.as_bytes());
        std::fs::write(
            release_path.with_extension("toml.sig"),
            hex::encode(release_signature.to_bytes()),
        )
        .expect("must write preferred release signature");

        let backend = select_metadata_backend(None, &layout).expect("configured backend must load");
        let outcome = run_search_command(&backend, "rip").expect("search must succeed");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].name, "ripgrep");
        assert_eq!(outcome.results[0].source, "fallback");
        assert_eq!(outcome.results[0].latest_version, "14.1.0");
        assert_eq!(outcome.skipped_packages.len(), 1);
        assert_eq!(outcome.skipped_packages[0].source, "preferred");
        assert_eq!(outcome.skipped_packages[0].package, "ripgrep");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn format_package_skip_warning_debug_escapes_all_fields() {
        let diagnostic = PackageSkipDiagnostic {
            package: "bad\"pkg".to_string(),
            reason_code: "package-metadata-invalid",
            source: "official\nsource".to_string(),
            detail: "line\n\"quoted\"".to_string(),
        };

        assert_eq!(
            format_package_skip_warning(&diagnostic),
            "warning: registry_package_skipped package=\"bad\\\"pkg\" reason=\"package-metadata-invalid\" source=\"official\\nsource\" detail=\"line\\n\\\"quoted\\\"\""
        );
    }

    #[test]
    fn best_available_short_description_prefers_manifest_description() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "ripgrep"
version = "14.1.0"
description = "Fast line-oriented search tool"
license = "MIT"
homepage = "https://ripgrep.example.test"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/ripgrep.tar.zst"
sha256 = "abc"
"#,
        )
        .expect("manifest should parse");

        let summary = best_available_short_description(&manifest);
        assert_eq!(summary.as_deref(), Some("Fast line-oriented search tool"));
    }

    #[test]
    fn best_available_short_description_sanitizes_tab_and_newline() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "ripgrep"
version = "14.1.0"
description = "Fast\tline\nsearch"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/ripgrep.tar.zst"
sha256 = "abc"
"#,
        )
        .expect("manifest should parse");

        let summary = best_available_short_description(&manifest);
        assert_eq!(summary.as_deref(), Some("Fast line search"));
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
    fn format_search_results_for_style_plain_preserves_contract() {
        let results = vec![SearchResult {
            name: "ripgrep".to_string(),
            description: Some("line search".to_string()),
            latest_version: "14.1.0".to_string(),
            source: "core".to_string(),
            match_kind: SearchMatchKind::Exact,
        }];

        assert_eq!(
            format_search_results_for_style(OutputStyle::Plain, &results, "rip"),
            vec![
                "name\tdescription\tlatest\tsource".to_string(),
                "ripgrep\tline search\t14.1.0\tcore".to_string(),
            ]
        );
    }

    #[test]
    fn format_search_results_for_style_rich_adds_summary_and_aligned_rows() {
        let results = vec![SearchResult {
            name: "ripgrep".to_string(),
            description: Some("line search".to_string()),
            latest_version: "14.1.0".to_string(),
            source: "core".to_string(),
            match_kind: SearchMatchKind::Exact,
        }];

        assert_eq!(
            format_search_results_for_style(OutputStyle::Rich, &results, "rip"),
            vec![
                "✓ 1 package matched 'rip'".to_string(),
                "name     description  latest  source".to_string(),
                "ripgrep  line search  14.1.0  core".to_string(),
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
                .join("releases")
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
    fn resolve_output_style_auto_uses_plain_when_stdout_is_tty_and_stderr_is_not() {
        assert_eq!(resolve_output_style(true, false), OutputStyle::Plain);
    }

    #[test]
    fn resolve_output_style_auto_uses_plain_when_stdout_is_not_tty() {
        assert_eq!(resolve_output_style(false, true), OutputStyle::Plain);
    }

    #[test]
    fn output_style_uses_stdout_for_result_formatting() {
        assert_eq!(resolve_output_style(true, false), OutputStyle::Plain);
        assert_eq!(resolve_output_style(true, true), OutputStyle::Rich);
    }

    #[test]
    fn progress_policy_uses_stderr_for_ephemeral_output() {
        assert!(!resolve_progress_enabled(OutputStyle::Plain, true));
        assert!(resolve_progress_enabled(OutputStyle::Rich, true));
        assert!(!resolve_progress_enabled(OutputStyle::Rich, false));
    }

    #[test]
    fn progress_mode_auto_follows_stderr_tty() {
        assert!(resolve_progress_mode(ProgressMode::Auto, OutputStyle::Rich, true));
        assert!(!resolve_progress_mode(
            ProgressMode::Auto,
            OutputStyle::Rich,
            false
        ));
        assert!(!resolve_progress_mode(
            ProgressMode::Auto,
            OutputStyle::Plain,
            true
        ));
    }

    #[test]
    fn progress_mode_always_forces_progress_for_rich_output() {
        assert!(resolve_progress_mode(
            ProgressMode::Always,
            OutputStyle::Rich,
            false
        ));
        assert!(!resolve_progress_mode(
            ProgressMode::Always,
            OutputStyle::Plain,
            false
        ));
    }

    #[test]
    fn progress_mode_never_disables_progress() {
        assert!(!resolve_progress_mode(
            ProgressMode::Never,
            OutputStyle::Rich,
            true
        ));
    }

    #[test]
    fn internal_ui_snapshot_forces_rich_output_without_changing_plain_rendering() {
        let _env_lock = ui_env_lock().lock().expect("ui env lock must be available");
        let _snapshot_guard = EnvVarGuard::set("CROSSPACK_INTERNAL_UI_SNAPSHOT", "1");

        assert_eq!(resolve_output_style(false, false), OutputStyle::Rich);
        assert_eq!(
            render_status_line(OutputStyle::Plain, "ok", "installed ripgrep"),
            "installed ripgrep"
        );
    }

    #[test]
    fn internal_terminal_width_reads_positive_width_only() {
        let _env_lock = ui_env_lock().lock().expect("ui env lock must be available");
        let _snapshot_guard = EnvVarGuard::set("CROSSPACK_INTERNAL_UI_SNAPSHOT", "1");
        let width_guard = EnvVarGuard::set("CROSSPACK_INTERNAL_TERM_WIDTH", "88");

        assert_eq!(internal_terminal_width(), Some(88));

        drop(width_guard);
        let _invalid_width_guard = EnvVarGuard::set("CROSSPACK_INTERNAL_TERM_WIDTH", "0");
        assert_eq!(internal_terminal_width(), None);
    }

    #[test]
    fn internal_terminal_width_is_ignored_outside_snapshot_mode() {
        let _env_lock = ui_env_lock().lock().expect("ui env lock must be available");
        let _width_guard = EnvVarGuard::set("CROSSPACK_INTERNAL_TERM_WIDTH", "88");

        assert_eq!(internal_terminal_width(), None);
    }

    #[test]
    fn internal_no_color_strips_rich_style_escape_sequences() {
        let _env_lock = ui_env_lock().lock().expect("ui env lock must be available");
        let _color_guard = EnvVarGuard::set("CROSSPACK_INTERNAL_NO_COLOR", "1");

        assert_eq!(colorize(section_style(), "Installed ripgrep"), "Installed ripgrep");
    }

    #[test]
    fn render_install_phase_message_includes_package_phase_and_step() {
        assert_eq!(
            render_install_phase_message("ripgrep", "download", 2, 7, Some((50, Some(200)))),
            "ripgrep download 2/7 50B/200B (25%)"
        );
    }

    #[test]
    fn render_install_phase_message_handles_unknown_download_total() {
        assert_eq!(
            render_install_phase_message("ripgrep", "download", 2, 7, Some((50, None))),
            "ripgrep download 2/7 50B"
        );
    }

    #[test]
    fn render_install_phase_message_omits_transfer_for_non_download_steps() {
        assert_eq!(
            render_install_phase_message("ripgrep", "verify", 3, 7, None),
            "ripgrep verify 3/7"
        );
    }

    #[test]
    fn render_install_phase_message_avoids_zero_total_steps() {
        assert_eq!(
            render_install_phase_message("ripgrep", "preflight", 0, 0, None),
            "ripgrep preflight 0/1"
        );
    }

    #[test]
    fn terminal_renderer_does_not_create_progress_for_plain_style() {
        let renderer = TerminalRenderer::from_style(OutputStyle::Plain);
        let progress = renderer.start_progress("install", 7);

        assert!(!progress.has_progress_bar_for_tests());
    }

    #[test]
    fn terminal_renderer_creates_progress_for_rich_style() {
        let renderer = TerminalRenderer::from_style(OutputStyle::Rich);
        let progress = renderer.start_progress("install", 7);

        assert!(progress.has_progress_bar_for_tests());
    }

    #[test]
    fn terminal_renderer_uses_label_once_in_initial_progress_state() {
        let renderer = TerminalRenderer::from_style(OutputStyle::Rich);
        let progress = renderer.start_progress("install", 7);

        assert_eq!(progress.progress_prefix_for_tests().as_deref(), Some("install"));
        assert_eq!(progress.progress_message_for_tests().as_deref(), Some(""));
    }

    #[test]
    fn internal_no_color_uses_unstyled_progress_template() {
        let _env_lock = ui_env_lock().lock().expect("ui env lock must be available");
        let _color_guard = EnvVarGuard::set("CROSSPACK_INTERNAL_NO_COLOR", "1");

        let template = progress_template_for_tests();

        assert_eq!(
            template,
            "{spinner} {prefix:<11} [{bar:20}] {pos:>2}/{len:2} {wide_msg}"
        );
    }

    #[test]
    fn progress_template_keeps_bar_before_variable_width_message() {
        let template = progress_template_for_tests();

        let bar_position = template.find("{bar:").expect("template should include bar");
        let message_position = template
            .find("{wide_msg}")
            .expect("template should include message");
        assert!(bar_position < message_position);
    }

    #[test]
    fn download_artifact_reports_progress_with_known_total() {
        let _env_lock = download_backend_env_lock()
            .lock()
            .expect("download backend env lock must be available");
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
        let _env_lock = download_backend_env_lock()
            .lock()
            .expect("download backend env lock must be available");
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
        let _env_lock = download_backend_env_lock()
            .lock()
            .expect("download backend env lock must be available");
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let cache_path = layout.prefix().join("download-cache-hit.bin");
        std::fs::write(&cache_path, b"cached").expect("must write cache fixture");

        let _backend_guard = DownloadBackendEnvGuard::set("not-a-backend");

        let status = download_artifact_with_progress(
            "https://example.test/cached.bin",
            &cache_path,
            false,
            |_downloaded, _total| {},
        )
        .expect("cache hit should short-circuit before backend validation");

        assert_eq!(status, "cache-hit");
        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn download_artifact_retries_in_process_download_before_succeeding() {
        let _env_lock = download_backend_env_lock()
            .lock()
            .expect("download backend env lock must be available");
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
    fn render_status_line_rich_includes_modern_marker() {
        assert_eq!(
            render_status_line(OutputStyle::Rich, "ok", "installed ripgrep 14.1.0"),
            "✓ installed ripgrep 14.1.0"
        );
    }

    #[test]
    fn render_status_line_rich_formats_warning() {
        assert_eq!(
            render_status_line(OutputStyle::Rich, "warn", "completion sync skipped"),
            "! completion sync skipped"
        );
    }

    #[test]
    fn render_compact_table_plain_uses_tabs() {
        let rows = vec![
            vec!["name".to_string(), "version".to_string()],
            vec!["ripgrep".to_string(), "14.1.0".to_string()],
        ];

        assert_eq!(
            render_compact_table(OutputStyle::Plain, &rows),
            vec!["name\tversion", "ripgrep\t14.1.0"]
        );
    }

    #[test]
    fn render_compact_table_rich_aligns_columns() {
        let rows = vec![
            vec!["name".to_string(), "version".to_string()],
            vec!["ripgrep".to_string(), "14.1.0".to_string()],
        ];

        assert_eq!(
            render_compact_table(OutputStyle::Rich, &rows),
            vec!["name     version", "ripgrep  14.1.0"]
        );
    }

    #[test]
    fn render_compact_table_rich_uses_display_width_for_unicode() {
        use pretty_assertions::assert_eq;

        let rows = vec![
            vec!["name".to_string(), "version".to_string()],
            vec!["工具".to_string(), "1.0.0".to_string()],
            vec!["ripgrep".to_string(), "14.1.0".to_string()],
        ];

        assert_eq!(
            render_compact_table(OutputStyle::Rich, &rows),
            vec![
                "name     version".to_string(),
                "工具     1.0.0".to_string(),
                "ripgrep  14.1.0".to_string(),
            ]
        );
    }

    #[test]
    fn internal_terminal_width_caps_rich_compact_table_lines() {
        let rows = vec![
            vec![
                "name".to_string(),
                "version".to_string(),
                "status".to_string(),
            ],
            vec![
                "ripgrep".to_string(),
                "14.1.0".to_string(),
                "installed".to_string(),
            ],
        ];

        assert_eq!(
            render_compact_table_with_width(render_compact_table(OutputStyle::Rich, &rows), Some(12)),
            vec!["name     ver".to_string(), "ripgrep  14.".to_string()]
        );
    }

    #[test]
    fn compact_table_width_cap_is_testable_without_global_env() {
        assert_eq!(
            render_compact_table_with_width(
                vec!["name     version".to_string(), "ripgrep  14.1.0".to_string()],
                Some(12),
            ),
            vec!["name     ver".to_string(), "ripgrep  14.".to_string()]
        );
    }

    #[test]
    fn terminal_snapshot_rich_status_gallery() {
        let output = [
            render_status_line(OutputStyle::Rich, "ok", "installed ripgrep 14.1.0"),
            render_status_line(OutputStyle::Rich, "warn", "completion sync skipped"),
            render_status_line(OutputStyle::Rich, "error", "source sync failed"),
            render_status_line(OutputStyle::Rich, "step", "cache: downloaded"),
        ]
        .join("\n");

        assert_terminal_snapshot("rich_status_gallery", output);
    }

    #[test]
    fn render_key_value_detail_rich_aligns_key() {
        assert_eq!(
            render_key_value_detail(OutputStyle::Rich, "snapshot", "abc123"),
            "  snapshot   abc123"
        );
    }

    #[test]
    fn render_key_value_detail_plain_uses_colon_separator() {
        assert_eq!(
            render_key_value_detail(OutputStyle::Plain, "snapshot", "abc123"),
            "snapshot: abc123"
        );
    }

    #[test]
    fn render_empty_state_plain_returns_message_only() {
        assert_eq!(
            render_empty_state(
                OutputStyle::Plain,
                "No installed packages",
                Some("Run `crosspack install <name>` to add one."),
            ),
            vec!["No installed packages"]
        );
    }

    #[test]
    fn render_empty_state_rich_includes_hint() {
        assert_eq!(
            render_empty_state(
                OutputStyle::Rich,
                "No installed packages",
                Some("Run `crosspack install <name>` to add one."),
            ),
            vec![
                "! No installed packages".to_string(),
                "• Run `crosspack install <name>` to add one.".to_string(),
            ]
        );
    }

    #[test]
    fn render_empty_state_rich_omits_missing_hint() {
        assert_eq!(
            render_empty_state(OutputStyle::Rich, "No installed packages", None),
            vec!["! No installed packages".to_string()]
        );
    }

    #[test]
    fn terminal_snapshot_rich_empty_state() {
        let output = render_empty_state(
            OutputStyle::Rich,
            "No installed packages",
            Some("Run `crosspack install <name>` to add one."),
        )
        .join("\n");

        assert_terminal_snapshot("rich_empty_state", output);
    }

    #[test]
    fn terminal_snapshot_rich_compact_table() {
        let rows = vec![
            vec!["name".to_string(), "version".to_string(), "status".to_string()],
            vec!["ripgrep".to_string(), "14.1.0".to_string(), "installed".to_string()],
            vec!["工具".to_string(), "1.0.0".to_string(), "available".to_string()],
        ];
        let output = render_compact_table(OutputStyle::Rich, &rows).join("\n");

        assert_terminal_snapshot("rich_compact_table", output);
    }

    #[test]
    fn terminal_snapshot_rich_search_output() {
        let results = vec![
            SearchResult {
                name: "ripgrep".to_string(),
                description: Some("Fast line-oriented search tool".to_string()),
                latest_version: "14.1.0".to_string(),
                source: "core".to_string(),
                match_kind: SearchMatchKind::Exact,
            },
            SearchResult {
                name: "roundrip".to_string(),
                description: None,
                latest_version: "0.9.0".to_string(),
                source: "community".to_string(),
                match_kind: SearchMatchKind::Prefix,
            },
        ];
        let output = format_search_results_for_style(OutputStyle::Rich, &results, "rip").join("\n");

        assert_terminal_snapshot("rich_search_output", output);
    }

    #[test]
    fn terminal_snapshot_rich_info_output() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "compiler"
version = "2.1.0"
description = "Portable toolchain"
license = "MIT OR Apache-2.0"
homepage = "https://example.test/compiler"
provides = ["c-compiler", "cc"]

[conflicts]
legacy-cc = "*"

[replaces]
old-cc = "<2.0.0"
"#,
        )
        .expect("manifest must parse");
        let output = format_info_lines_for_style(OutputStyle::Rich, "compiler", &[manifest]).join("\n");

        assert_terminal_snapshot("rich_info_output", output);
    }

    #[test]
    fn terminal_snapshot_rich_registry_status_output() {
        let sources = vec![
            RegistrySourceWithSnapshotState {
                source: RegistrySourceRecord {
                    name: "core".to_string(),
                    kind: RegistrySourceKind::Git,
                    location: "https://github.com/spiritledsoftware/crosspack-registry.git"
                        .to_string(),
                    fingerprint_sha256:
                        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                            .to_string(),
                    enabled: true,
                    priority: 100,
                    community: None,
                },
                snapshot: RegistrySourceSnapshotState::Ready {
                    snapshot_id: "git:0123456789abcdef".to_string(),
                },
            },
            RegistrySourceWithSnapshotState {
                source: RegistrySourceRecord {
                    name: "edge".to_string(),
                    kind: RegistrySourceKind::Filesystem,
                    location: "/opt/crosspack/edge-registry".to_string(),
                    fingerprint_sha256:
                        "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
                            .to_string(),
                    enabled: true,
                    priority: 10,
                    community: None,
                },
                snapshot: RegistrySourceSnapshotState::Error {
                    status: RegistrySourceWithSnapshotStatus::Unreadable,
                    reason_code: "snapshot-unreadable".to_string(),
                },
            },
        ];
        let output = format_registry_list_status_lines(OutputStyle::Rich, sources).join("\n");

        assert_terminal_snapshot("rich_registry_status_output", output);
    }

    #[test]
    fn terminal_snapshot_rich_update_output() {
        let report = sample_update_report();
        let plan = plan_update_output(&report, OutputStyle::Rich);
        let output = plan
            .lines
            .into_iter()
            .chain(std::iter::once(plan.summary_line))
            .collect::<Vec<_>>()
            .join("\n");

        assert_terminal_snapshot("rich_update_output", output);
    }

    #[test]
    fn dispatch_output_style_formats_pin_status_lines_with_strict_split() {
        let requirement = VersionReq::parse("^14").expect("pin requirement should parse");
        let pin_path = Path::new("/tmp/crosspack/state/pins/ripgrep.pin");

        let plain = format_pin_status_lines(OutputStyle::Plain, "ripgrep", &requirement, pin_path);
        assert_eq!(
            plain,
            vec![
                "pinned ripgrep to ^14".to_string(),
                format!("pin: {}", pin_path.display()),
            ]
        );

        let rich = format_pin_status_lines(OutputStyle::Rich, "ripgrep", &requirement, pin_path);
        assert_eq!(rich[0], "✓ pinned ripgrep to ^14");
        assert_eq!(rich[1], format!("• pin: {}", pin_path.display()));
    }

    #[test]
    fn dispatch_output_style_formats_registry_add_lines_with_strict_split() {
        let plain = format_registry_add_status_lines(
            OutputStyle::Plain,
            "core",
            "git",
            100,
            "0123456789abcdef0123456789abcdef",
        );
        assert_eq!(plain[0], "added registry core");
        assert_eq!(plain[1], "kind: git");
        assert_eq!(plain[2], "priority: 100");
        assert_eq!(plain[3], "fingerprint: 0123456789abcdef...");

        let rich = format_registry_add_status_lines(
            OutputStyle::Rich,
            "core",
            "git",
            100,
            "0123456789abcdef0123456789abcdef",
        );
        assert_eq!(rich[0], "✓ added registry core");
        assert_eq!(rich[1], "• kind: git");
        assert_eq!(rich[2], "• priority: 100");
        assert_eq!(rich[3], "• fingerprint: 0123456789abcdef...");
    }

    #[test]
    fn bundle_output_style_formats_export_status_line_with_strict_split() {
        let bundle_path = Path::new("/tmp/crosspack.bundle.toml");

        let plain = format_bundle_export_status_line(OutputStyle::Plain, bundle_path);
        assert_eq!(plain, format!("bundle exported: {}", bundle_path.display()));

        let rich = format_bundle_export_status_line(OutputStyle::Rich, bundle_path);
        assert_eq!(
            rich,
            format!("✓ bundle exported: {}", bundle_path.display())
        );
    }

    #[test]
    fn bundle_output_style_keeps_bundle_document_payload_undecorated() {
        let bundle = BundleDocument {
            format: BUNDLE_FORMAT_MARKER.to_string(),
            version: BUNDLE_FORMAT_VERSION,
            roots: vec![BundleRoot {
                name: "ripgrep".to_string(),
                target: None,
                requirement: Some("^14".to_string()),
            }],
            snapshot_context: None,
        };

        let rendered = render_bundle_document(&bundle).expect("bundle document should render");
        assert!(
            rendered.contains("format = \"crosspack.bundle\""),
            "bundle payload should remain raw document bytes"
        );
        assert!(
            !rendered.contains("[OK]")
                && !rendered.contains("[..]")
                && !rendered.contains("[WARN]")
                && !rendered.contains("[ERR]"),
            "bundle payload must remain undecorated"
        );
    }

    #[test]
    fn renderer_progress_line_rich_suppresses_zero_total_completion_line() {
        let line = render_progress_line(
            OutputStyle::Rich,
            "update",
            0,
            0,
            Some(std::time::Duration::from_millis(250)),
        );

        assert!(
            line.is_none(),
            "rich progress completion line must be suppressed when total is zero"
        );
    }

    #[test]
    fn renderer_terminal_progress_exposes_progress_safe_status_and_line_emitters() {
        let _print_status: fn(&TerminalProgress, &str, &str) = TerminalProgress::print_status;
        let _print_line: fn(&TerminalProgress, &str) = TerminalProgress::print_line;
    }

    #[test]
    fn render_rich_install_detail_row_is_structured_and_badge_free() {
        let cases = [
            ("step", "archive", "tar.zst"),
            ("ok", "resolved", "ripgrep 14.1.0 for x86_64-unknown-linux-gnu"),
            ("warn", "warning", "signature metadata missing"),
        ];

        for (status, key, value) in cases {
            let line = render_rich_install_detail_row(status, key, value);
            let columns = line.split_whitespace().collect::<Vec<_>>();
            let expected_columns = std::iter::once(key)
                .chain(value.split_whitespace())
                .collect::<Vec<_>>();

            assert_eq!(columns, expected_columns);
            assert!(!line.contains('|'));
            assert!(
                !line.contains('✓') && !line.contains('!') && !line.contains('×'),
                "rich install detail row must avoid status markers: {line}"
            );
            assert!(
                !line.contains("[OK]")
                    && !line.contains("[..]")
                    && !line.contains("[ERR]")
                    && !line.contains("[WARN]"),
                "rich install detail row must avoid plain status badges: {line}"
            );
        }
    }

    #[test]
    fn update_output_progress_guard_skips_zero_work_totals() {
        assert!(!should_render_progress(0));
        assert!(!should_render_progress(1));
        assert!(should_render_progress(2));
    }

    #[test]
    fn plan_update_output_disables_progress_for_zero_line_report_and_keeps_summary_contract() {
        let report = empty_update_report();

        let plan = plan_update_output(&report, OutputStyle::Plain);

        assert!(plan.lines.is_empty());
        assert!(!plan.render_progress);
        assert_eq!(
            plan.summary_line,
            "update summary: updated=0 up-to-date=0 failed=0"
        );
    }

    #[test]
    fn plan_update_output_disables_progress_for_single_source_report() {
        let report = UpdateReport {
            lines: vec!["core: up-to-date".to_string()],
            updated: 0,
            up_to_date: 1,
            failed: 0,
        };

        let plan = plan_update_output(&report, OutputStyle::Plain);

        assert!(!plan.render_progress);
        assert_eq!(
            plan.summary_line,
            "update summary: updated=0 up-to-date=1 failed=0"
        );
    }

    #[test]
    fn plan_update_output_enables_progress_for_multi_source_report() {
        let report = sample_update_report();

        let plan = plan_update_output(&report, OutputStyle::Plain);

        assert!(plan.render_progress);
    }

    #[test]
    fn plan_update_output_matches_plain_contract_and_decorates_rich_lines() {
        let report = sample_update_report();

        let plain_plan = plan_update_output(&report, OutputStyle::Plain);
        assert_eq!(plain_plan.lines, report.lines);

        let rich_plan = plan_update_output(&report, OutputStyle::Rich);
        assert_eq!(rich_plan.lines[0], "✓ core: updated (snapshot=git:abc)");
        assert_eq!(
            rich_plan.lines[1],
            "• mirror: up-to-date (snapshot=git:abc)"
        );
        assert_eq!(
            rich_plan.lines[2],
            "× edge: failed (reason=source-sync-failed)"
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
        assert_eq!(lines[0], "✓ core: updated (snapshot=git:abc)");
        assert_eq!(lines[1], "• mirror: up-to-date (snapshot=git:abc)");
        assert_eq!(lines[2], "× edge: failed (reason=source-sync-failed)");
    }

    #[test]
    fn format_install_outcome_lines_plain_matches_existing_contract() {
        use pretty_assertions::assert_eq;

        let outcome = sample_install_outcome();
        let lines = format_install_outcome_lines(&outcome, OutputStyle::Plain);
        assert_eq!(
            lines,
            vec![
                "resolved ripgrep 14.1.0 for x86_64-unknown-linux-gnu".to_string(),
                "archive: tar.zst".to_string(),
                "artifact: https://example.test/ripgrep-14.1.0.tar.zst".to_string(),
                "cache: /tmp/crosspack/cache/ripgrep/14.1.0/artifact.tar.zst (downloaded)"
                    .to_string(),
                "install_root: /tmp/crosspack/pkgs/ripgrep/14.1.0".to_string(),
                "exposed_bins: rg".to_string(),
                "exposed_completions: bash:rg".to_string(),
                "exposed_gui_assets: app:dev.ripgrep.viewer".to_string(),
                "native_gui_records: app:dev.ripgrep.viewer".to_string(),
                "receipt: /tmp/crosspack/state/installed/ripgrep.receipt".to_string(),
            ]
        );
    }

    #[test]
    fn format_rich_install_outcome_lines_are_structured_and_badge_free() {
        let mut outcome = sample_install_outcome();
        outcome
            .warnings
            .push("native registration skipped".to_string());

        let lines = format_rich_install_outcome_lines(&outcome);

        assert!(
            lines.iter().all(|line| !line.contains('|')),
            "rich install detail rows must avoid table chrome: {lines:?}"
        );
        assert!(
            lines.iter().all(|line| {
                !line.contains("[OK]")
                    && !line.contains("[..]")
                    && !line.contains("[ERR]")
                    && !line.contains("[WARN]")
            }),
            "rich install detail rows must avoid plain status badges: {lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.starts_with("warning")),
            "rich install detail rows must preserve warning key: {lines:?}"
        );
    }

    #[test]
    fn format_install_outcome_lines_rich_adds_step_indicators() {
        let outcome = sample_install_outcome();
        let lines = format_install_outcome_lines(&outcome, OutputStyle::Rich);
        assert!(lines.iter().any(|line| line.contains("receipt: ")));
    }

    #[test]
    fn format_install_outcome_lines_rich_does_not_include_plain_status_badges() {
        let outcome = sample_install_outcome();
        let lines = format_install_outcome_lines(&outcome, OutputStyle::Rich);
        assert!(
            lines
                .iter()
                .all(|line| !line.contains("[OK]") && !line.contains("[..]")),
            "rich install outcome details must avoid plain status badges: {lines:?}"
        );
    }

    #[test]
    fn terminal_snapshot_rich_install_outcome() {
        let output = format_rich_install_outcome_lines(&sample_install_outcome()).join("\n");

        assert_terminal_snapshot("rich_install_outcome", output);
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

    #[test]
    fn install_resolved_writes_legacy_receipt_and_state_document() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut resolved = resolved_install("demo-bin", "1.0.0");
        resolved.archive_type = ArchiveType::Bin;
        resolved.artifact.url = "https://example.test/demo-bin".to_string();
        resolved.artifact.sha256 = EMPTY_SHA256.to_string();
        resolved.artifact.archive = Some("bin".to_string());
        resolved.artifact.binaries = vec![crosspack_core::ArtifactBinary {
            name: "demo-bin".to_string(),
            path: "demo-bin".to_string(),
        }];
        resolved.manifest.services = vec![ServiceDeclaration {
            name: "demo-bin".to_string(),
            native_id: None,
        }];
        let cache_path = resolved_artifact_cache_path(
            &layout,
            &resolved.manifest.name,
            &resolved.manifest.version.to_string(),
            &resolved.resolved_target,
            resolved.archive_type,
            &resolved.artifact.url,
        )
        .expect("must resolve bin cache path");
        std::fs::create_dir_all(cache_path.parent().expect("cache path must have parent"))
            .expect("must create cache dir");
        std::fs::write(cache_path, b"").expect("must seed cached bin artifact");
        let install_plan = build_install_plan_from_resolved(
            PlanOperation::Install,
            Some(resolved.resolved_target.clone()),
            std::slice::from_ref(&resolved),
            &[],
            &[RootInstallRequest {
                name: "demo-bin".to_string(),
                requirement: VersionReq::STAR,
            }],
        );

        install_resolved(
            &layout,
            &resolved,
            &["demo-bin@1.0.0".to_string()],
            InstallResolvedPlanContext {
                root_names: &["demo-bin".to_string()],
                install_plan: &install_plan,
                planned_dependency_overrides: &HashMap::new(),
            },
            InstallResolvedOptions {
                snapshot_id: None,
                force_redownload: false,
                interaction_policy: InstallInteractionPolicy::default(),
                progress_enabled: false,
            },
            None,
        )
        .expect("install should succeed");

        let identity = InstalledPackageIdentity {
            profile: "default".to_string(),
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            source_namespace: "default".to_string(),
            source_provenance: Some("unknown".to_string()),
            package: "demo-bin".to_string(),
        };
        assert!(layout.receipt_path("demo-bin").exists());
        assert!(layout.identity_receipt_path(&identity).exists());
        assert!(layout.identity_package_dir(&identity, "1.0.0").exists());
        assert!(layout
            .installed_identity_state_document_path(&InstalledPackageIdentity {
                profile: "default".to_string(),
                target: Some("x86_64-unknown-linux-gnu".to_string()),
                source_namespace: "default".to_string(),
                source_provenance: Some("unknown".to_string()),
                package: "demo-bin".to_string(),
            })
            .exists());
        let state = read_installed_package_state(&layout, "demo-bin")
            .expect("must read installed state")
            .expect("demo-bin must be installed");
        assert_eq!(state.receipt.name, "demo-bin");
        assert_eq!(state.receipt.exposed_bins, vec!["demo-bin"]);
        assert_eq!(
            read_declared_services_state(&layout, "demo-bin")
                .expect("must read package-keyed declared services")
                .len(),
            1
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn service_activation_transaction_install_fails_closed_before_host_mutation() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let integration = PackageIntegration::Service {
            name: "caddy".to_string(),
            linux_systemd_user: Some("services/caddy.service".to_string()),
            macos_launch_agent: None,
            windows_service: None,
            enable: true,
        };
        let projections = projected_integrations("caddy", &integration)
            .expect("must project service integration");
        write_integration_state(&layout, "caddy", &projections)
            .expect("must seed projected integration state");
        let host = HostActivationContext::linux()
            .with_prefix("/prefix")
            .with_home("/home/user");
        let err = activate_enabled_services_for_install(
            &layout,
            "caddy",
            "default--x86_64-unknown-linux-gnu--core--caddy",
            &host,
            std::slice::from_ref(&integration),
            &projections,
        )
        .expect_err("production install-time service activation should fail closed");

        assert!(
            err.to_string().contains("service activation failed"),
            "unexpected error: {err}"
        );
        let records = read_integration_activation_state(&layout).expect("must read activation state");
        assert!(
            records.is_empty(),
            "fail-closed service enable must not persist applied activation state"
        );
        assert!(
            std::fs::read_dir(layout.transactions_dir())
                .map(|entries| {
                    entries.filter_map(|entry| entry.ok()).all(|entry| {
                        entry.path().extension().and_then(|ext| ext.to_str()) != Some("journal")
                    })
                })
                .unwrap_or(true),
            "fail-closed service enable must not journal synthetic metadata rollback"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn service_activation_transaction_install_rejects_existing_service_state_before_apply() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let integration = PackageIntegration::Service {
            name: "caddy".to_string(),
            linux_systemd_user: Some("services/caddy.service".to_string()),
            macos_launch_agent: None,
            windows_service: None,
            enable: true,
        };
        let projections = projected_integrations("caddy", &integration)
            .expect("must project service integration");
        write_integration_state(&layout, "caddy", &projections)
            .expect("must seed projected integration state");
        write_integration_activation_state(
            &layout,
            &[IntegrationActivationRecord {
                package_state_key: "default--x86_64-unknown-linux-gnu--core--caddy".to_string(),
                package: "caddy".to_string(),
                integration_key: "service:caddy".to_string(),
                kind: "service".to_string(),
                adapter: IntegrationAdapterKind::SystemdUser,
                scope: IntegrationActivationScope::User,
                desired_state: IntegrationDesiredState::Running,
                applied_state: IntegrationAppliedState::Running,
                host_path: Some("systemd-user:caddy.service".to_string()),
                reason_code: IntegrationReasonCode::Ok,
            }],
        )
        .expect("must seed existing activation state");
        let host = HostActivationContext::linux()
            .with_prefix("/prefix")
            .with_home("/home/user");
        let err = activate_enabled_services_for_install(
            &layout,
            "caddy",
            "default--x86_64-unknown-linux-gnu--core--caddy",
            &host,
            std::slice::from_ref(&integration),
            &projections,
        )
        .expect_err("existing service activation should be rejected before replacement");

        assert!(
            err.to_string().contains("host-path-conflict"),
            "unexpected error: {err}"
        );
        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn service_activation_transaction_failure_leaves_no_activation_journal_or_state() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let integration = PackageIntegration::Service {
            name: "caddy".to_string(),
            linux_systemd_user: Some("services/caddy.service".to_string()),
            macos_launch_agent: None,
            windows_service: None,
            enable: true,
        };
        let projections = projected_integrations("caddy", &integration)
            .expect("must project service integration");
        write_integration_state(&layout, "caddy", &projections)
            .expect("must seed projected integration state");
        let host = HostActivationContext::linux()
            .with_prefix("/prefix")
            .with_home("/home/user");
        let mut txid = String::new();

        let err = execute_with_transaction(&layout, "install", None, |tx| {
            txid = tx.txid.clone();
            activate_enabled_services_for_install(
                &layout,
                "caddy",
                "default--x86_64-unknown-linux-gnu--core--caddy",
                &host,
                std::slice::from_ref(&integration),
                &projections,
            )
        })
        .expect_err("service enable should fail closed inside transaction");

        assert!(
            err.to_string().contains("service activation failed"),
            "unexpected error: {err}"
        );
        assert!(
            read_integration_activation_state(&layout)
                .expect("must read activation state")
                .is_empty()
        );
        let journal_path = layout.transaction_journal_path(&txid);
        let journal = std::fs::read_to_string(&journal_path).unwrap_or_default();
        assert!(
            !journal.contains("RemoveCreatedServiceMetadata"),
            "synthetic service rollback must not be journaled: {journal}"
        );
        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[cfg(not(windows))]
    #[test]
    fn service_activation_transaction_failure_removes_copied_launch_agent() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let prefix = layout.prefix().to_path_buf();
        let source_path = prefix
            .join("share")
            .join("integrations")
            .join("agents")
            .join("caddy.plist");
        std::fs::create_dir_all(source_path.parent().expect("source must have parent"))
            .expect("must create source parent");
        std::fs::write(&source_path, b"<plist><dict/></plist>").expect("must write source plist");
        let home = prefix.join("home").join("user");
        let launch_agent = home
            .join("Library")
            .join("LaunchAgents")
            .join("caddy.plist");
        let integration = PackageIntegration::Service {
            name: "caddy".to_string(),
            linux_systemd_user: None,
            macos_launch_agent: Some("agents/caddy.plist".to_string()),
            windows_service: None,
            enable: true,
        };
        let projections = projected_integrations("caddy", &integration)
            .expect("must project service integration");
        write_integration_state(&layout, "caddy", &projections)
            .expect("must seed projected integration state");
        let host = HostActivationContext::macos()
            .with_prefix(prefix.to_str().expect("prefix must be utf-8"))
            .with_home(home.to_str().expect("home must be utf-8"));

        let err = activate_enabled_services_for_install(
            &layout,
            "caddy",
            "default--aarch64-apple-darwin--core--caddy",
            &host,
            std::slice::from_ref(&integration),
            &projections,
        )
        .expect_err("launchctl failure should fail closed");

        assert!(
            err.to_string().contains("service activation failed"),
            "unexpected error: {err}"
        );
        assert!(
            !launch_agent.exists(),
            "failed install-time activation must remove copied launch agent"
        );
        assert!(
            read_integration_activation_state(&layout)
                .expect("must read activation state")
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ambiguous_installed_package_name_blocks_uninstall_with_identity_guidance() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut linux_receipt = install_receipt("demo", "1.0.0", InstallReason::Root, &[]);
        linux_receipt.target = Some("x86_64-unknown-linux-gnu".to_string());
        let linux_state = InstalledPackageState {
            identity: InstalledPackageIdentity::from_legacy_receipt(&linux_receipt),
            version: linux_receipt.version.clone(),
            receipt: linux_receipt,
            gui_assets: Vec::new(),
            native_gui_records: Vec::new(),
            services: Vec::new(),
            integrations: Vec::new(),
        };
        write_installed_package_state(&layout, &linux_state).expect("must write linux state");

        let mut macos_receipt = install_receipt("demo", "1.0.0", InstallReason::Root, &[]);
        macos_receipt.target = Some("aarch64-apple-darwin".to_string());
        let macos_state = InstalledPackageState {
            identity: InstalledPackageIdentity::from_legacy_receipt(&macos_receipt),
            version: macos_receipt.version.clone(),
            receipt: macos_receipt,
            gui_assets: Vec::new(),
            native_gui_records: Vec::new(),
            services: Vec::new(),
            integrations: Vec::new(),
        };
        write_installed_package_state(&layout, &macos_state).expect("must write macos state");

        let err = run_uninstall_command(&layout, "demo".to_string())
            .expect_err("ambiguous package should fail before uninstall");
        let message = err.to_string();
        assert!(
            message.contains("installed package name 'demo' is ambiguous; specify one of:"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains("demo --target aarch64-apple-darwin --profile default --source default")
                && message.contains(
                    "demo --target x86_64-unknown-linux-gnu --profile default --source default"
                ),
            "error should list matching selectors: {message}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
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
        let install_plan = build_install_plan_from_resolved(
            PlanOperation::Install,
            Some(resolved.resolved_target.clone()),
            std::slice::from_ref(&resolved),
            &[],
            &[],
        );

        let err = install_resolved(
            &layout,
            &resolved,
            &[],
            InstallResolvedPlanContext {
                root_names: &[],
                install_plan: &install_plan,
                planned_dependency_overrides: &HashMap::new(),
            },
            InstallResolvedOptions {
                snapshot_id: None,
                force_redownload: false,
                interaction_policy: InstallInteractionPolicy::default(),
                progress_enabled: false,
            },
            None,
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
        let install_plan = build_install_plan_from_resolved(
            PlanOperation::Install,
            Some(resolved.resolved_target.clone()),
            std::slice::from_ref(&resolved),
            &[],
            &[],
        );

        let err = install_resolved(
            &layout,
            &resolved,
            &[],
            InstallResolvedPlanContext {
                root_names: &[],
                install_plan: &install_plan,
                planned_dependency_overrides: &HashMap::new(),
            },
            InstallResolvedOptions {
                snapshot_id: None,
                force_redownload: false,
                interaction_policy: InstallInteractionPolicy::default(),
                progress_enabled: false,
            },
            None,
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
    fn integration_projection_sync_replaces_stale_state() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let install_root = layout.package_dir("kubectx", "0.9.5");
        fs::create_dir_all(&install_root).expect("must create install root");
        fs::write(install_root.join("kubectl-ctx"), b"#!/bin/sh\n").expect("must write plugin");

        let stale = IntegrationProjection {
            kind: "path_plugin".to_string(),
            key: "path_plugin:kubectl:old".to_string(),
            rel_path: "path-plugins/kubectl/kubectl-old".to_string(),
        };
        fs::create_dir_all(layout.integrations_dir().join("path-plugins/kubectl"))
            .expect("must create stale integration dir");
        fs::write(layout.integrations_dir().join(&stale.rel_path), b"stale")
            .expect("must write stale integration");
        let identity = test_installed_identity("kubectx");
        write_identity_integration_state(&layout, &identity, std::slice::from_ref(&stale))
            .expect("must seed stale integration state");

        let integration = PackageIntegration::PathPlugin {
            host: "kubectl".to_string(),
            name: "ctx".to_string(),
            source: "kubectl-ctx".to_string(),
        };
        let projected = sync_integration_projection_state(
            &layout,
            "kubectx",
            &identity,
            &install_root,
            std::slice::from_ref(&integration),
            &[],
        )
        .expect("must sync integrations");

        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].rel_path, "path-plugins/kubectl/kubectl-ctx");
        assert!(layout.integrations_dir().join(&projected[0].rel_path).exists());
        assert!(!layout.integrations_dir().join(&stale.rel_path).exists());
        assert_eq!(
            read_identity_integration_state(&layout, &identity).expect("must read identity state"),
            projected
        );
        assert_eq!(
            read_integration_state(&layout, "kubectx").expect("must read legacy mirror state"),
            projected
        );
    }

    #[test]
    fn integration_projection_sync_rejects_other_package_owner() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let install_root = layout.package_dir("other-compose", "1.0.0");
        fs::create_dir_all(&install_root).expect("must create install root");
        fs::write(install_root.join("artifact.bin"), b"#!/bin/sh\n").expect("must write plugin");

        let owned = IntegrationProjection {
            kind: "docker_cli_plugin".to_string(),
            key: "docker_cli_plugin:compose".to_string(),
            rel_path: "docker/cli-plugins/docker-compose".to_string(),
        };
        write_integration_state(&layout, "docker-compose", std::slice::from_ref(&owned))
            .expect("must seed other package integration state");

        let integration = PackageIntegration::DockerCliPlugin {
            name: "compose".to_string(),
            source: "artifact.bin".to_string(),
        };
        let err = sync_integration_projection_state(
            &layout,
            "other-compose",
            &test_installed_identity("other-compose"),
            &install_root,
            std::slice::from_ref(&integration),
            &[],
        )
        .expect_err("other package ownership should block projection");

        assert!(err.to_string().contains("already owned by package 'docker-compose'"));
    }

    #[test]
    fn integration_projection_sync_projects_shell_init_and_rejects_foreign_owner() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let install_root = layout.package_dir("starship", "1.0.0");
        fs::create_dir_all(&install_root).expect("must create install root");
        let identity = test_installed_identity("starship");
        let shell_init = crosspack_core::PackageShellInit {
            name: "starship".to_string(),
            binary: "starship".to_string(),
            strategy: crosspack_core::ShellInitStrategy::EvalStdout,
            bash: Some(vec!["init".to_string(), "bash".to_string()]),
            zsh: None,
            fish: None,
            powershell: None,
        };

        let projected_integrations = sync_integration_projection_state(
            &layout,
            "starship",
            &identity,
            &install_root,
            &[],
            std::slice::from_ref(&shell_init),
        )
        .expect("must sync shell init");

        assert!(projected_integrations.is_empty());
        let projected_shell_init =
            read_shell_init_state(&layout, "starship").expect("must read shell init state");
        assert_eq!(projected_shell_init.len(), 1);
        assert_eq!(
            projected_shell_init[0].rel_path,
            "shell/init/bash/starship/starship.sh"
        );
        assert!(layout
            .share_dir()
            .join(&projected_shell_init[0].rel_path)
            .exists());

        write_shell_init_state(&layout, "other-starship", &projected_shell_init)
            .expect("must seed foreign owner");
        let err = sync_integration_projection_state(
            &layout,
            "starship",
            &identity,
            &install_root,
            &[],
            std::slice::from_ref(&shell_init),
        )
        .expect_err("foreign shell init owner should block projection");
        assert!(err.to_string().contains("already owned by package"));
    }

    #[test]
    fn integration_list_lines_report_projected_state_from_sidecars() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_integration_state(
            &layout,
            "kubectx",
            &[IntegrationProjection {
                kind: "path_plugin".to_string(),
                key: "path_plugin:kubectl:ctx".to_string(),
                rel_path: "path-plugins/kubectl/kubectl-ctx".to_string(),
            }],
        )
        .expect("must seed integration state");

        let rows = collect_projected_integration_rows(&layout).expect("must collect integrations");
        let lines = format_projected_integration_lines(&rows);

        assert_eq!(
            lines,
            vec!["integration package=kubectx name=ctx key=path_plugin:kubectl:ctx kind=path_plugin state=projected adapter=none reason=not-enabled path=path-plugins/kubectl/kubectl-ctx"],
        );
    }

    #[test]
    fn integrations_projected_line_percent_encodes_rel_path_with_spaces_without_activation() {
        let row = ProjectedIntegrationRow {
            package: "demo".to_string(),
            name: "tool".to_string(),
            key: "path_plugin:demo:tool".to_string(),
            kind: "path_plugin".to_string(),
            rel_path: "path-plugins/demo/my tool".to_string(),
            activation: None,
        };

        let line = format_projected_integration_lines(&[row])
            .into_iter()
            .next()
            .expect("must format projected row");
        let fields = line
            .split_whitespace()
            .filter_map(|field| field.split_once('='))
            .collect::<HashMap<_, _>>();

        assert_eq!(fields.get("state"), Some(&"projected"));
        assert_eq!(fields.get("adapter"), Some(&"none"));
        assert_eq!(fields.get("path"), Some(&"path-plugins/demo/my%20tool"));
    }

    #[test]
    fn integration_status_line_accepts_full_key_or_short_name() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_integration_state(
            &layout,
            "docker-compose",
            &[IntegrationProjection {
                kind: "docker_cli_plugin".to_string(),
                key: "docker_cli_plugin:compose".to_string(),
                rel_path: "docker/cli-plugins/docker-compose".to_string(),
            }],
        )
        .expect("must seed integration state");

        let short = integration_status_line(&layout, "docker-compose", "compose")
            .expect("must resolve short integration name");
        let full = integration_status_line(&layout, "docker-compose", "docker_cli_plugin:compose")
            .expect("must resolve full integration key");

        assert_eq!(short, full);
        assert_eq!(
            short,
            "integration package=docker-compose name=compose key=docker_cli_plugin:compose kind=docker_cli_plugin state=projected adapter=none reason=not-enabled path=docker/cli-plugins/docker-compose",
        );
    }

    #[test]
    fn integrations_enable_disable_lines_use_activation_state_key_order() {
        let projection = IntegrationProjection {
            kind: "docker_cli_plugin".to_string(),
            key: "docker_cli_plugin:compose".to_string(),
            rel_path: "docker/cli-plugins/docker-compose".to_string(),
        };
        let enabled = IntegrationActivationRecord {
            package_state_key: "default--x86_64-unknown-linux-gnu--core--docker-compose".to_string(),
            package: "docker-compose".to_string(),
            integration_key: "docker_cli_plugin:compose".to_string(),
            kind: "docker_cli_plugin".to_string(),
            adapter: IntegrationAdapterKind::DockerCli,
            scope: IntegrationActivationScope::None,
            desired_state: IntegrationDesiredState::Enabled,
            applied_state: IntegrationAppliedState::Enabled,
            host_path: Some("/home/user/.docker/cli-plugins/docker-compose".to_string()),
            reason_code: IntegrationReasonCode::Ok,
        };
        let disabled = IntegrationActivationRecord {
            desired_state: IntegrationDesiredState::Projected,
            applied_state: IntegrationAppliedState::Projected,
            ..enabled.clone()
        };

        assert_eq!(
            format_integration_activation_line("docker-compose", &projection, Some(&enabled)),
            "integration package=docker-compose name=compose key=docker_cli_plugin:compose kind=docker_cli_plugin state=enabled adapter=docker-cli reason=ok path=/home/user/.docker/cli-plugins/docker-compose"
        );
        assert_eq!(
            format_integration_activation_line("docker-compose", &projection, Some(&disabled)),
            "integration package=docker-compose name=compose key=docker_cli_plugin:compose kind=docker_cli_plugin state=projected adapter=docker-cli reason=ok path=/home/user/.docker/cli-plugins/docker-compose"
        );
    }

    #[test]
    fn integrations_windows_path_output_stays_key_value_parseable() {
        let projection = IntegrationProjection {
            kind: "docker_cli_plugin".to_string(),
            key: "docker_cli_plugin:compose".to_string(),
            rel_path: "docker/cli-plugins/docker-compose".to_string(),
        };
        let activation = IntegrationActivationRecord {
            package_state_key: "default--x86_64-pc-windows-msvc--core--docker-compose".to_string(),
            package: "docker-compose".to_string(),
            integration_key: "docker_cli_plugin:compose".to_string(),
            kind: "docker_cli_plugin".to_string(),
            adapter: IntegrationAdapterKind::DockerCli,
            scope: IntegrationActivationScope::None,
            desired_state: IntegrationDesiredState::Enabled,
            applied_state: IntegrationAppliedState::Enabled,
            host_path: Some("C:\\Users\\Ada\\.docker\\cli-plugins\\docker-compose".to_string()),
            reason_code: IntegrationReasonCode::Ok,
        };

        let line = format_integration_activation_line("docker-compose", &projection, Some(&activation));
        let fields = line
            .split_whitespace()
            .filter_map(|field| field.split_once('='))
            .collect::<HashMap<_, _>>();

        assert_eq!(fields.get("state"), Some(&"enabled"));
        assert_eq!(fields.get("adapter"), Some(&"docker-cli"));
        assert_eq!(fields.get("reason"), Some(&"ok"));
        assert_eq!(
            fields.get("path"),
            Some(&"C:%5CUsers%5CAda%5C.docker%5Ccli-plugins%5Cdocker-compose")
        );
    }

    #[test]
    fn integrations_path_with_spaces_is_percent_encoded_for_key_value_output() {
        let projection = IntegrationProjection {
            kind: "docker_cli_plugin".to_string(),
            key: "docker_cli_plugin:compose".to_string(),
            rel_path: "docker/cli-plugins/docker-compose".to_string(),
        };
        let activation = IntegrationActivationRecord {
            package_state_key: "default--x86_64-unknown-linux-gnu--core--docker-compose".to_string(),
            package: "docker-compose".to_string(),
            integration_key: "docker_cli_plugin:compose".to_string(),
            kind: "docker_cli_plugin".to_string(),
            adapter: IntegrationAdapterKind::DockerCli,
            scope: IntegrationActivationScope::None,
            desired_state: IntegrationDesiredState::Enabled,
            applied_state: IntegrationAppliedState::Enabled,
            host_path: Some("/home/ada/Docker Plugins/docker-compose".to_string()),
            reason_code: IntegrationReasonCode::Ok,
        };

        let line = format_integration_activation_line("docker-compose", &projection, Some(&activation));
        let fields = line
            .split_whitespace()
            .filter_map(|field| field.split_once('='))
            .collect::<HashMap<_, _>>();

        assert_eq!(
            fields.get("path"),
            Some(&"/home/ada/Docker%20Plugins/docker-compose")
        );
    }

    #[test]
    fn integrations_short_name_ambiguity_requires_full_keys_for_enable() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_integration_state(
            &layout,
            "demo",
            &[
                IntegrationProjection {
                    kind: "docker_cli_plugin".to_string(),
                    key: "docker_cli_plugin:compose".to_string(),
                    rel_path: "docker/cli-plugins/docker-compose".to_string(),
                },
                IntegrationProjection {
                    kind: "path_plugin".to_string(),
                    key: "path_plugin:docker:compose".to_string(),
                    rel_path: "path-plugins/docker/docker-compose".to_string(),
                },
            ],
        )
        .expect("must seed integration state");

        let err = resolve_projected_integration(&layout, "demo", "compose")
            .expect_err("ambiguous short name should fail");

        assert!(err.to_string().contains("use full integration key"));
    }

    #[test]
    fn integrations_path_plugin_enable_derives_host_binary_name_from_full_key() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "kubectx".to_string(),
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
        .expect("must write receipt");
        write_integration_state(
            &layout,
            "kubectx",
            &[IntegrationProjection {
                kind: "path_plugin".to_string(),
                key: "path_plugin:kubectl:ctx".to_string(),
                rel_path: "path-plugins/kubectl/kubectl-ctx".to_string(),
            }],
        )
        .expect("must seed integration state");

        let host = HostActivationContext::linux()
            .with_prefix("/prefix")
            .with_home("/home/user");
        let mut fs = MemoryActivationFs::new(HostPlatform::Linux);
        let line = run_integration_activation_command_with_fs(
            &layout,
            &host,
            &mut fs,
            "kubectx",
            "ctx",
            true,
        )
        .expect("path plugin enable should succeed through fake installer fs");

        assert!(line.contains("state=enabled adapter=path-plugin-bin reason=ok"));
        assert!(line.contains("path=/prefix/bin/kubectl-ctx"));
    }

    #[test]
    fn integration_list_reports_man_page_projection_and_enable_is_unsupported() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "delta".to_string(),
                version: "0.18.2".to_string(),
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
        .expect("must write receipt");
        write_integration_state(
            &layout,
            "delta",
            &[IntegrationProjection {
                kind: "man_page".to_string(),
                key: "man_page:1:delta".to_string(),
                rel_path: "man/man1/delta.1".to_string(),
            }],
        )
        .expect("must seed integration state");

        let rows = collect_projected_integration_rows(&layout).expect("must collect integrations");
        assert_eq!(
            format_projected_integration_lines(&rows),
            vec!["integration package=delta name=delta key=man_page:1:delta kind=man_page state=projected adapter=none reason=not-enabled path=man/man1/delta.1"],
        );

        let err = run_integration_activation_command(&layout, "delta", "delta", true)
            .expect_err("man page activation should be unsupported");
        assert!(
            err.to_string()
                .contains("integration activation is not supported for kind 'man_page'")
        );
        assert!(
            read_integration_activation_state(&layout)
                .expect("must read activation state")
                .is_empty(),
            "unsupported man page activation must not persist activation state"
        );
    }

    #[test]
    fn integrations_activation_failure_persists_status_record_without_false_ok() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "docker-compose".to_string(),
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
        .expect("must write receipt");
        write_integration_state(
            &layout,
            "docker-compose",
            &[IntegrationProjection {
                kind: "docker_cli_plugin".to_string(),
                key: "docker_cli_plugin:compose".to_string(),
                rel_path: "docker/cli-plugins/docker-compose".to_string(),
            }],
        )
        .expect("must seed integration state");

        let host = HostActivationContext::linux()
            .with_prefix("/prefix")
            .with_home("/home/user");
        let mut fs = MemoryActivationFs::new(HostPlatform::Linux);
        fs.write_file("/home/user/.docker/cli-plugins/docker-compose", b"host-owned");

        let line = run_integration_activation_command_with_fs(
            &layout,
            &host,
            &mut fs,
            "docker-compose",
            "compose",
            true,
        )
        .expect("conflict should render deterministic line");

        assert!(line.contains("state=projected adapter=docker-cli reason=host-path-conflict"));
        let records = read_integration_activation_state(&layout).expect("must read activation state");
        assert_eq!(records.len(), 1, "failed activation should persist status record");
        assert_eq!(records[0].reason_code, IntegrationReasonCode::HostPathConflict);
        assert_eq!(records[0].applied_state, IntegrationAppliedState::Failed);
    }

    #[test]
    fn integrations_disable_failure_preserves_failure_applied_state() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let projection = IntegrationProjection {
            kind: "docker_cli_plugin".to_string(),
            key: "docker_cli_plugin:compose".to_string(),
            rel_path: "docker/cli-plugins/docker-compose".to_string(),
        };
        let plan = IntegrationActivationPlan {
            package_state_key: "default--x86_64-unknown-linux-gnu--core--docker-compose".to_string(),
            package: "docker-compose".to_string(),
            integration_key: projection.key.clone(),
            kind: projection.kind.clone(),
            adapter: IntegrationAdapterKind::DockerCli,
            scope: IntegrationActivationScope::User,
            desired_state: IntegrationDesiredState::Projected,
            host_path: "/home/user/.docker/cli-plugins/docker-compose".to_string(),
            source_path: "/prefix/share/integrations/docker/cli-plugins/docker-compose".to_string(),
        };

        let line = finish_integration_activation_command(
            &layout,
            "docker-compose",
            &projection,
            &plan,
            ActivationAdapterOutcome {
                reason_code: IntegrationReasonCode::HostPathConflict,
                applied_state: IntegrationAppliedState::Failed,
                rollback: Vec::new(),
            },
            false,
        )
        .expect("disable failure should render deterministic line");

        assert!(line.contains("state=projected adapter=docker-cli reason=host-path-conflict"));
        let records = read_integration_activation_state(&layout).expect("must read activation state");
        assert_eq!(records[0].desired_state, IntegrationDesiredState::Projected);
        assert_eq!(records[0].applied_state, IntegrationAppliedState::Failed);
    }

    #[test]
    fn integrations_activation_success_persists_activation_state() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "docker-compose".to_string(),
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
        .expect("must write receipt");
        write_integration_state(
            &layout,
            "docker-compose",
            &[IntegrationProjection {
                kind: "docker_cli_plugin".to_string(),
                key: "docker_cli_plugin:compose".to_string(),
                rel_path: "docker/cli-plugins/docker-compose".to_string(),
            }],
        )
        .expect("must seed integration state");

        let host = HostActivationContext::linux()
            .with_prefix("/prefix")
            .with_home("/home/user");
        let mut fs = MemoryActivationFs::new(HostPlatform::Linux);
        let line = run_integration_activation_command_with_fs(
            &layout,
            &host,
            &mut fs,
            "docker-compose",
            "compose",
            true,
        )
        .expect("fake activation should succeed");

        assert!(line.contains("state=enabled adapter=docker-cli reason=ok"));
        let records = read_integration_activation_state(&layout).expect("must read activation state");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].integration_key, "docker_cli_plugin:compose");
        assert_eq!(records[0].applied_state, IntegrationAppliedState::Enabled);
    }

    #[test]
    fn integrations_enable_rejects_active_transaction_before_mutation() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        set_active_transaction(&layout, "tx-existing").expect("must seed active transaction");

        let err = run_integration_activation_command(&layout, "docker-compose", "compose", true)
            .expect_err("active transaction should block explicit activation");

        assert!(err.to_string().contains("active_transaction"));
    }

    #[test]
    fn integrations_enable_journals_rollback_intent_before_success() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "docker-compose".to_string(),
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
        .expect("must write receipt");
        write_integration_state(
            &layout,
            "docker-compose",
            &[IntegrationProjection {
                kind: "docker_cli_plugin".to_string(),
                key: "docker_cli_plugin:compose".to_string(),
                rel_path: "docker/cli-plugins/docker-compose".to_string(),
            }],
        )
        .expect("must seed integration state");

        let host = HostActivationContext::linux()
            .with_prefix("/prefix")
            .with_home("/home/user");
        let mut line = None;
        execute_with_transaction(&layout, "integrations", None, |tx| {
            let mut fs = MemoryActivationFs::new(HostPlatform::Linux);
            line = Some(run_integration_activation_command_with_fs_and_tx(
                &layout,
                Some(tx),
                &host,
                &mut fs,
                "docker-compose",
                "compose",
                true,
            )?);
            Ok(())
        })
        .expect("transactional activation should succeed");

        assert!(line.expect("must render line").contains("reason=ok"));
        let journals = std::fs::read_dir(layout.transactions_dir())
            .expect("must read transaction dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("journal"))
            .map(|entry| std::fs::read_to_string(entry.path()).expect("must read journal"))
            .collect::<Vec<_>>();
        assert!(
            journals
                .iter()
                .any(|journal| journal.contains("integration_activation_rollback")),
            "activation transaction should journal rollback intent"
        );
    }

    #[test]
    fn service_status_lines_use_package_service_activation_state_key_order() {
        let activation = IntegrationActivationRecord {
            package_state_key: "default--x86_64-unknown-linux-gnu--core--caddy".to_string(),
            package: "caddy".to_string(),
            integration_key: "service:caddy".to_string(),
            kind: "service".to_string(),
            adapter: IntegrationAdapterKind::SystemdUser,
            scope: IntegrationActivationScope::User,
            desired_state: IntegrationDesiredState::Running,
            applied_state: IntegrationAppliedState::Running,
            host_path: Some("systemd-user:caddy.service".to_string()),
            reason_code: IntegrationReasonCode::Ok,
        };

        assert_eq!(
            format_service_activation_line("caddy", "caddy", &activation, true),
            "service package=caddy name=caddy state=running adapter=systemd-user scope=user applied=true reason=ok"
        );

        let failed = IntegrationActivationRecord {
            adapter: IntegrationAdapterKind::LaunchdUser,
            applied_state: IntegrationAppliedState::Unsupported,
            reason_code: IntegrationReasonCode::AdapterToolMissing,
            ..activation
        };
        assert_eq!(
            format_service_activation_line("caddy", "caddy", &failed, false),
            "service package=caddy name=caddy state=unsupported adapter=launchd-user scope=user applied=false reason=adapter-tool-missing"
        );
    }

    #[test]
    fn service_status_line_for_running_activation_reports_applied_true() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "caddy".to_string(),
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
        .expect("must write receipt");
        write_declared_services_state(
            &layout,
            "caddy",
            &[ServiceDeclaration {
                name: "caddy".to_string(),
                native_id: None,
            }],
        )
        .expect("must write service declaration");
        write_integration_activation_state(
            &layout,
            &[IntegrationActivationRecord {
                package_state_key: "default--x86_64-unknown-linux-gnu--core--caddy".to_string(),
                package: "caddy".to_string(),
                integration_key: "service:caddy".to_string(),
                kind: "service".to_string(),
                adapter: IntegrationAdapterKind::SystemdUser,
                scope: IntegrationActivationScope::User,
                desired_state: IntegrationDesiredState::Running,
                applied_state: IntegrationAppliedState::Running,
                host_path: Some("systemd-user:caddy.service".to_string()),
                reason_code: IntegrationReasonCode::Ok,
            }],
        )
        .expect("must seed activation state");

        let line = service_status_line_for_package_from_state(&layout, "caddy", "caddy")
            .expect("status should render activation state");

        assert_eq!(
            line,
            "service package=caddy name=caddy state=running adapter=systemd-user scope=user applied=true reason=ok"
        );
    }

    #[test]
    fn services_status_resolves_projected_service_without_declared_sidecar() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_install_receipt(
            &layout,
            &install_receipt("syncthing", "1.0.0", InstallReason::Root, &[]),
        )
        .expect("must write receipt");
        write_integration_state(
            &layout,
            "syncthing",
            &[IntegrationProjection {
                kind: "service".to_string(),
                key: "service:syncthing".to_string(),
                rel_path: "services/syncthing/syncthing.service".to_string(),
            }],
        )
        .expect("must seed service projection state");

        let line = service_status_line_for_package_from_state(&layout, "syncthing", "syncthing")
            .expect("status should resolve service projection");

        assert_eq!(
            line,
            "service package=syncthing name=syncthing state=projected adapter=none scope=user applied=false reason=not-enabled"
        );
    }

    #[test]
    fn services_status_uses_resolved_projection_activation_key() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_install_receipt(
            &layout,
            &install_receipt("syncthing", "1.0.0", InstallReason::Root, &[]),
        )
        .expect("must write receipt");
        write_integration_state(
            &layout,
            "syncthing",
            &[IntegrationProjection {
                kind: "service".to_string(),
                key: "systemd-user:service:syncthing".to_string(),
                rel_path: "services/syncthing/syncthing.service".to_string(),
            }],
        )
        .expect("must seed service projection state");
        write_integration_activation_state(
            &layout,
            &[IntegrationActivationRecord {
                package_state_key: "default--x86_64-unknown-linux-gnu--core--syncthing"
                    .to_string(),
                package: "syncthing".to_string(),
                integration_key: "systemd-user:service:syncthing".to_string(),
                kind: "service".to_string(),
                adapter: IntegrationAdapterKind::SystemdUser,
                scope: IntegrationActivationScope::User,
                desired_state: IntegrationDesiredState::Running,
                applied_state: IntegrationAppliedState::Running,
                host_path: Some("systemd-user:syncthing.service".to_string()),
                reason_code: IntegrationReasonCode::Ok,
            }],
        )
        .expect("must seed activation state");

        let line = service_status_line_for_package(&layout, "syncthing", "syncthing", |plan| {
            assert_eq!(plan.integration_key, "systemd-user:service:syncthing");
            ActivationAdapterOutcome {
                reason_code: IntegrationReasonCode::Ok,
                applied_state: IntegrationAppliedState::Running,
                rollback: Vec::new(),
            }
        })
        .expect("status should resolve activation by projection key");

        assert_eq!(
            line,
            "service package=syncthing name=syncthing state=running adapter=systemd-user scope=user applied=true reason=ok"
        );
    }

    #[test]
    fn services_list_includes_projected_services_without_duplicating_legacy_services() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        for name in ["caddy", "syncthing"] {
            write_install_receipt(
                &layout,
                &install_receipt(name, "1.0.0", InstallReason::Root, &[]),
            )
            .expect("must write receipt");
        }
        write_declared_services_state(
            &layout,
            "caddy",
            &[ServiceDeclaration {
                name: "caddy".to_string(),
                native_id: None,
            }],
        )
        .expect("must seed legacy service declaration");
        write_integration_state(
            &layout,
            "caddy",
            &[IntegrationProjection {
                kind: "service".to_string(),
                key: "service:caddy".to_string(),
                rel_path: "services/caddy/caddy.service".to_string(),
            }],
        )
        .expect("must seed duplicate service projection");
        write_integration_state(
            &layout,
            "syncthing",
            &[IntegrationProjection {
                kind: "service".to_string(),
                key: "service:syncthing".to_string(),
                rel_path: "services/syncthing/syncthing.service".to_string(),
            }],
        )
        .expect("must seed projection-only service");

        let rows = collect_managed_service_rows(&layout).expect("must collect services");
        let rendered = rows
            .iter()
            .map(format_managed_service_row)
            .collect::<Vec<_>>();

        assert_eq!(
            rendered,
            vec![
                "service package=caddy name=caddy state=projected adapter=none scope=user applied=false reason=not-enabled",
                "service package=syncthing name=syncthing state=projected adapter=none scope=user applied=false reason=not-enabled",
            ]
        );
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn services_start_projected_service_persists_running_activation() {
        let layout = test_layout();
        let _cleanup = TestLayoutCleanup::new(&layout);
        layout.ensure_base_dirs().expect("must create dirs");
        write_install_receipt(
            &layout,
            &install_receipt("syncthing", "1.0.0", InstallReason::Root, &[]),
        )
        .expect("must write receipt");
        let (rel_path, expected_adapter) = if cfg!(target_os = "macos") {
            (
                "services/syncthing/syncthing.launchd.plist",
                IntegrationAdapterKind::LaunchdUser,
            )
        } else {
            (
                "services/syncthing/syncthing.service",
                IntegrationAdapterKind::SystemdUser,
            )
        };
        write_integration_state(
            &layout,
            "syncthing",
            &[IntegrationProjection {
                kind: "service".to_string(),
                key: "service:syncthing".to_string(),
                rel_path: rel_path.to_string(),
            }],
        )
        .expect("must seed service projection state");

        let line = service_action_line_for_package(
            &layout,
            "syncthing",
            "syncthing",
            NativeServiceAction::Start,
            |plan, source| {
                assert_eq!(source, ServiceActionPlanSource::ProjectedActivation);
                assert_eq!(plan.kind, "service");
                assert_eq!(plan.integration_key, "service:syncthing");
                assert_eq!(plan.adapter, expected_adapter);
                ActivationAdapterOutcome {
                    reason_code: IntegrationReasonCode::Ok,
                    applied_state: IntegrationAppliedState::Running,
                    rollback: Vec::new(),
                }
            },
        )
        .expect("start should activate projected service");

        assert_eq!(
            line,
            format!(
                "service package=syncthing name=syncthing state=running adapter={} scope=user applied=true reason=ok",
                expected_adapter.as_str()
            )
        );
        let records = read_integration_activation_state(&layout).expect("must read activation state");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].integration_key, "service:syncthing");
        assert_eq!(records[0].desired_state, IntegrationDesiredState::Running);
        assert_eq!(records[0].applied_state, IntegrationAppliedState::Running);
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn services_restart_projected_service_persists_running_activation() {
        let layout = test_layout();
        let _cleanup = TestLayoutCleanup::new(&layout);
        layout.ensure_base_dirs().expect("must create dirs");
        write_install_receipt(
            &layout,
            &install_receipt("syncthing", "1.0.0", InstallReason::Root, &[]),
        )
        .expect("must write receipt");
        let (rel_path, expected_adapter) = if cfg!(target_os = "macos") {
            (
                "services/syncthing/syncthing.launchd.plist",
                IntegrationAdapterKind::LaunchdUser,
            )
        } else {
            (
                "services/syncthing/syncthing.service",
                IntegrationAdapterKind::SystemdUser,
            )
        };
        write_integration_state(
            &layout,
            "syncthing",
            &[IntegrationProjection {
                kind: "service".to_string(),
                key: "service:syncthing".to_string(),
                rel_path: rel_path.to_string(),
            }],
        )
        .expect("must seed service projection state");

        let line = service_action_line_for_package(
            &layout,
            "syncthing",
            "syncthing",
            NativeServiceAction::Restart,
            |plan, source| {
                assert_eq!(source, ServiceActionPlanSource::ProjectedActivation);
                assert_eq!(plan.kind, "service");
                assert_eq!(plan.integration_key, "service:syncthing");
                assert_eq!(plan.adapter, expected_adapter);
                ActivationAdapterOutcome {
                    reason_code: IntegrationReasonCode::Ok,
                    applied_state: IntegrationAppliedState::Running,
                    rollback: Vec::new(),
                }
            },
        )
        .expect("restart should activate projected service");

        assert_eq!(
            line,
            format!(
                "service package=syncthing name=syncthing state=running adapter={} scope=user applied=true reason=ok",
                expected_adapter.as_str()
            )
        );
        let records = read_integration_activation_state(&layout).expect("must read activation state");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].integration_key, "service:syncthing");
        assert_eq!(records[0].desired_state, IntegrationDesiredState::Running);
        assert_eq!(records[0].applied_state, IntegrationAppliedState::Running);
    }

    #[test]
    fn services_stop_projected_service_without_activation_does_not_persist_state() {
        let layout = test_layout();
        let _cleanup = TestLayoutCleanup::new(&layout);
        layout.ensure_base_dirs().expect("must create dirs");
        write_install_receipt(
            &layout,
            &install_receipt("syncthing", "1.0.0", InstallReason::Root, &[]),
        )
        .expect("must write receipt");
        write_integration_state(
            &layout,
            "syncthing",
            &[IntegrationProjection {
                kind: "service".to_string(),
                key: "service:syncthing".to_string(),
                rel_path: "services/syncthing/syncthing.service".to_string(),
            }],
        )
        .expect("must seed service projection state");

        let line = service_action_line_for_package(
            &layout,
            "syncthing",
            "syncthing",
            NativeServiceAction::Stop,
            |_, _| panic!("stop without activation should not call adapter"),
        )
        .expect("stop should render projected service state");

        assert_eq!(
            line,
            "service package=syncthing name=syncthing state=projected adapter=none scope=user applied=false reason=not-enabled"
        );
        assert!(
            read_integration_activation_state(&layout)
                .expect("must read activation state")
                .is_empty(),
            "stop of projected service must not create activation state"
        );
    }

    #[test]
    fn service_start_without_activation_record_reports_non_ok_and_does_not_noop() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "caddy".to_string(),
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
        .expect("must write receipt");
        write_declared_services_state(
            &layout,
            "caddy",
            &[ServiceDeclaration {
                name: "caddy".to_string(),
                native_id: None,
            }],
        )
        .expect("must write service declaration");

        let line = service_action_line_for_package(
            &layout,
            "caddy",
            "caddy",
            NativeServiceAction::Start,
            |_, _| ActivationAdapterOutcome {
                reason_code: IntegrationReasonCode::StateMissing,
                applied_state: IntegrationAppliedState::Unsupported,
                rollback: Vec::new(),
            },
        )
        .expect("service action should render deterministic non-ok line");

        assert_eq!(
            line,
            "service package=caddy name=caddy state=unsupported adapter=none scope=user applied=false reason=state-missing"
        );
    }

    #[test]
    fn service_start_updates_activation_state_after_adapter_success() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "caddy".to_string(),
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
        .expect("must write receipt");
        write_declared_services_state(
            &layout,
            "caddy",
            &[ServiceDeclaration {
                name: "caddy".to_string(),
                native_id: None,
            }],
        )
        .expect("must write service declaration");
        write_integration_activation_state(
            &layout,
            &[IntegrationActivationRecord {
                package_state_key: "default--x86_64-unknown-linux-gnu--core--caddy".to_string(),
                package: "caddy".to_string(),
                integration_key: "service:caddy".to_string(),
                kind: "service".to_string(),
                adapter: IntegrationAdapterKind::SystemdUser,
                scope: IntegrationActivationScope::User,
                desired_state: IntegrationDesiredState::Projected,
                applied_state: IntegrationAppliedState::Stopped,
                host_path: Some("systemd-user:caddy.service".to_string()),
                reason_code: IntegrationReasonCode::Ok,
            }],
        )
        .expect("must seed activation state");

        let line = service_action_line_for_package(
            &layout,
            "caddy",
            "caddy",
            NativeServiceAction::Start,
            |_, source| {
                assert_eq!(source, ServiceActionPlanSource::ExistingActivation);
                ActivationAdapterOutcome {
                reason_code: IntegrationReasonCode::Ok,
                applied_state: IntegrationAppliedState::Running,
                rollback: Vec::new(),
                }
            },
        )
        .expect("service start should render updated line");

        assert_eq!(
            line,
            "service package=caddy name=caddy state=running adapter=systemd-user scope=user applied=true reason=ok"
        );
        let records = read_integration_activation_state(&layout).expect("must read activation state");
        assert_eq!(records[0].applied_state, IntegrationAppliedState::Running);
        assert_eq!(records[0].desired_state, IntegrationDesiredState::Running);
    }

    #[test]
    fn service_action_journals_adapter_rollback_payload() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_install_receipt(
            &layout,
            &install_receipt("caddy", "1.0.0", InstallReason::Root, &[]),
        )
        .expect("must write receipt");
        write_declared_services_state(
            &layout,
            "caddy",
            &[ServiceDeclaration {
                name: "caddy".to_string(),
                native_id: None,
            }],
        )
        .expect("must write service declaration");
        write_integration_activation_state(
            &layout,
            &[IntegrationActivationRecord {
                package_state_key: "default--x86_64-unknown-linux-gnu--core--caddy".to_string(),
                package: "caddy".to_string(),
                integration_key: "service:caddy".to_string(),
                kind: "service".to_string(),
                adapter: IntegrationAdapterKind::SystemdUser,
                scope: IntegrationActivationScope::User,
                desired_state: IntegrationDesiredState::Running,
                applied_state: IntegrationAppliedState::Running,
                host_path: Some("systemd-user:caddy.service".to_string()),
                reason_code: IntegrationReasonCode::Ok,
            }],
        )
        .expect("must seed activation state");
        let rollback = ActivationRollbackEntry {
            operation: ActivationRollbackOperation::RemoveCreatedServiceMetadata,
            path: "systemd-user:caddy.service".to_string(),
            previous_symlink_target: None,
            previous_shim_target: None,
            previous_owner: None,
            created_symlink_target: None,
            created_shim_target: None,
            created_owner: None,
            expected_current_symlink_target: None,
            expected_current_shim_target: None,
            expected_current_owner: None,
            expected_current_absent: false,
            created_parent_dirs: Vec::new(),
        };
        let mut txid = String::new();

        execute_with_transaction(&layout, "services", None, |tx| {
            txid = tx.txid.clone();
            let line = service_action_line_for_package_with_tx(
                &layout,
                Some(tx),
                "caddy",
                "caddy",
                NativeServiceAction::Stop,
                |_, source| {
                    assert_eq!(source, ServiceActionPlanSource::ExistingActivation);
                    ActivationAdapterOutcome {
                        reason_code: IntegrationReasonCode::Ok,
                        applied_state: IntegrationAppliedState::Stopped,
                        rollback: vec![rollback.clone()],
                    }
                },
            )?;
            assert_eq!(
                line,
                "service package=caddy name=caddy state=stopped adapter=systemd-user scope=user applied=true reason=ok"
            );
            Ok(())
        })
        .expect("transactional service action should succeed");

        let journal = std::fs::read_to_string(layout.transaction_journal_path(&txid))
            .expect("must read service action journal");
        assert!(journal.contains("integration_activation_rollback"));
        assert!(journal.contains("RemoveCreatedServiceMetadata"));
    }

    #[test]
    fn service_action_command_rejects_active_transaction_before_state_write() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        set_active_transaction(&layout, "tx-service-active").expect("must write active marker");

        let err = run_service_action_for_package_command(
            &layout,
            "caddy",
            "caddy",
            NativeServiceAction::Start,
        )
        .expect_err("active transaction should block service mutation");

        assert!(
            err.to_string().contains("cannot services")
                && err.to_string().contains("active_transaction"),
            "unexpected error: {err}"
        );
        assert!(
            read_integration_activation_state(&layout)
                .expect("must read activation state")
                .is_empty(),
            "blocked service action must not write activation state"
        );
    }

    #[test]
    fn service_projection_status_treats_duplicate_platform_files_as_one_integration() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_integration_state(
            &layout,
            "caddy",
            &[
                IntegrationProjection {
                    kind: "service".to_string(),
                    key: "service:caddy".to_string(),
                    rel_path: "services/caddy/caddy.service".to_string(),
                },
                IntegrationProjection {
                    kind: "service".to_string(),
                    key: "service:caddy".to_string(),
                    rel_path: "services/caddy/caddy.launchd.plist".to_string(),
                },
            ],
        )
        .expect("must seed multi-platform service projections");

        let projection = resolve_projected_integration(&layout, "caddy", "service:caddy")
            .expect("duplicate platform files should resolve as one logical service integration");

        assert_eq!(projection.key, "service:caddy");
        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn integration_activation_test_helper_dispatches_service_runner() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_install_receipt(
            &layout,
            &install_receipt("caddy", "1.0.0", InstallReason::Root, &[]),
        )
        .expect("must write receipt");
        let integration = PackageIntegration::Service {
            name: "caddy".to_string(),
            linux_systemd_user: Some("services/caddy.service".to_string()),
            macos_launch_agent: None,
            windows_service: None,
            enable: false,
        };
        write_integration_state(
            &layout,
            "caddy",
            &projected_integrations("caddy", &integration).expect("must project service"),
        )
        .expect("must seed integration state");
        let host = HostActivationContext::linux()
            .with_prefix("/prefix")
            .with_home("/home/user");
        let mut fs = MemoryActivationFs::new(HostPlatform::Linux);
        let mut called = false;

        let line = run_integration_activation_command_with_fs_tx_and_service_runner(
            &layout,
            None,
            &host,
            &mut fs,
            "caddy",
            "service:caddy",
            true,
            |plan, enable| {
                called = true;
                assert_eq!(plan.kind, "service");
                assert!(enable);
                ActivationAdapterOutcome {
                    reason_code: IntegrationReasonCode::Ok,
                    applied_state: IntegrationAppliedState::Running,
                    rollback: Vec::new(),
                }
            },
        )
        .expect("service integration activation should use injected service runner");

        assert!(called, "service activation should call injected runner");
        assert!(line.contains("state=running adapter=systemd-user reason=ok"));
        let records = read_integration_activation_state(&layout).expect("must read state");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].applied_state, IntegrationAppliedState::Running);
        assert_eq!(records[0].reason_code, IntegrationReasonCode::Ok);
        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn integration_status_line_reports_ambiguous_short_names_with_full_key_guidance() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        write_integration_state(
            &layout,
            "kubectl-plugins",
            &[
                IntegrationProjection {
                    kind: "path_plugin".to_string(),
                    key: "path_plugin:kubectl:ctx".to_string(),
                    rel_path: "path-plugins/kubectl/kubectl-ctx".to_string(),
                },
                IntegrationProjection {
                    kind: "path_plugin".to_string(),
                    key: "path_plugin:helm:ctx".to_string(),
                    rel_path: "path-plugins/helm/helm-ctx".to_string(),
                },
            ],
        )
        .expect("must seed integration state");

        let rows = collect_projected_integration_rows(&layout).expect("must collect integrations");
        let lines = format_projected_integration_lines(&rows);
        assert!(
            lines
                .iter()
                .any(|line| line.contains("key=path_plugin:kubectl:ctx")),
            "list output should expose the kubectl full key"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("key=path_plugin:helm:ctx")),
            "list output should expose the helm full key"
        );

        let err = integration_status_line(&layout, "kubectl-plugins", "ctx")
            .expect_err("short name should be ambiguous");
        assert!(
            err.to_string().contains("use full integration key"),
            "unexpected error: {err}"
        );

        let full = integration_status_line(&layout, "kubectl-plugins", "path_plugin:helm:ctx")
            .expect("full key should resolve ambiguous short name");
        assert_eq!(
            full,
            "integration package=kubectl-plugins name=ctx key=path_plugin:helm:ctx kind=path_plugin state=projected adapter=none reason=not-enabled path=path-plugins/helm/helm-ctx",
        );
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

    #[test]
    fn source_build_metadata_requires_build_from_source_flag_when_binary_artifact_missing() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "demo"
version = "1.0.0"

[[artifacts]]
target = "aarch64-apple-darwin"
url = "https://example.test/demo-1.0.0-aarch64.tar.zst"
sha256 = "abc123"

[source_build]
url = "https://example.test/demo-1.0.0-src.tar.gz"
archive_sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
build_system = "cargo"
build_commands = ["cargo", "build", "--release"]
install_commands = ["cargo", "install", "--path", "."]
"#,
        )
        .expect("manifest should parse");

        let err = select_artifact_for_target(&manifest, "x86_64-unknown-linux-gnu", false)
            .expect_err("source-build gate should require explicit opt-in");
        assert_eq!(
            err.to_string(),
            "source build required for demo 1.0.0 on target x86_64-unknown-linux-gnu: no binary artifact published; rerun with --build-from-source"
        );
    }

    #[test]
    fn source_build_metadata_with_flag_uses_source_build_path() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "demo"
version = "1.0.0"

[[artifacts]]
target = "aarch64-apple-darwin"
url = "https://example.test/demo-1.0.0-aarch64.tar.zst"
sha256 = "abc123"

[source_build]
url = "https://example.test/demo-1.0.0-src.tar.gz"
archive_sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
build_system = "cargo"
build_commands = ["cargo", "build", "--release"]
install_commands = ["cargo", "install", "--path", "."]
"#,
        )
        .expect("manifest should parse");

        let (selected, source_build) =
            select_install_plan_for_target(&manifest, "x86_64-unknown-linux-gnu", true)
                .expect("source-build opt-in should use supported source-build path");
        assert_eq!(
            selected.target, "aarch64-apple-darwin",
            "fallback artifact metadata should be selected deterministically"
        );
        let source_build = source_build.expect("source-build plan should be present");
        assert_eq!(
            source_build.url,
            "https://example.test/demo-1.0.0-src.tar.gz"
        );
        assert_eq!(
            source_build.archive_sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(source_build.archive_type, ArchiveType::TarGz);
        assert_eq!(source_build.build_system, "cargo");
    }

    #[test]
    fn upgrade_build_from_source_opt_in_unblocks_source_only_upgrade_resolution() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        configure_ready_source(&layout, "official");

        let host_target = host_target_triple();
        let other_target = if host_target == "x86_64-unknown-linux-gnu" {
            "aarch64-apple-darwin"
        } else {
            "x86_64-unknown-linux-gnu"
        };
        write_signed_source_build_metadata_manifest(
            &layout,
            "official",
            "demo",
            "2.0.0",
            other_target,
        );
        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "demo".to_string(),
                version: "1.0.0".to_string(),
                dependencies: Vec::new(),
                target: Some(host_target.to_string()),
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
        .expect("must write installed receipt");

        let err = run_upgrade_command(
            &layout,
            None,
            Some("demo".to_string()),
            UpgradeCommandOptions {
                dry_run: true,
                explain: false,
                build_from_source: false,
                provider_overrides: &BTreeMap::new(),
                interaction_policy: InstallInteractionPolicy::default(),
            },
        )
        .expect_err("upgrade should require explicit source-build opt-in");
        assert!(
            err.to_string().contains("rerun with --build-from-source"),
            "unexpected error: {err}"
        );

        run_upgrade_command(
            &layout,
            None,
            Some("demo".to_string()),
            UpgradeCommandOptions {
                dry_run: true,
                explain: false,
                build_from_source: true,
                provider_overrides: &BTreeMap::new(),
                interaction_policy: InstallInteractionPolicy::default(),
            },
        )
        .expect("upgrade dry-run should resolve source-build install plan when opted in");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn source_build_metadata_rejects_unknown_fields_fail_closed() {
        let err = PackageManifest::from_toml_str(
            r#"
name = "demo"
version = "1.0.0"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/demo-1.0.0.tar.zst"
sha256 = "abc123"

[source_build]
url = "https://example.test/demo-1.0.0-src.zip"
archive_sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
build_system = "shell"
build_commands = ["sh", "-c", "true"]
install_commands = ["sh", "-c", "true"]
unexpected = "value"
"#,
        )
        .expect_err("unknown source_build fields must be rejected");
        let rendered = err
            .chain()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            rendered.contains("unknown field") && rendered.contains("unexpected"),
            "unexpected parse error: {rendered}"
        );
    }

    #[test]
    fn source_build_metadata_rejects_invalid_archive_sha256_fail_closed() {
        let target = host_target_triple().to_string();
        let manifest = PackageManifest::from_toml_str(&format!(
            r#"
name = "demo"
version = "1.0.0"

[[artifacts]]
target = "{target}"
url = "https://example.test/demo-1.0.0.tar.zst"
sha256 = "abc123"

[source_build]
url = "https://example.test/demo-1.0.0-src.tar.gz"
archive_sha256 = "xyz"
build_system = "shell"
build_commands = ["sh", "-c", "true"]
install_commands = ["sh", "-c", "true"]
"#
        ))
        .expect("manifest should parse before source-build plan validation");

        let err = select_install_plan_for_target(&manifest, &target, true)
            .expect_err("invalid source archive checksum metadata must fail closed");
        assert!(
            err.to_string()
                .contains("archive_sha256 must be a 64-character hexadecimal SHA-256 digest"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn source_build_metadata_rejects_empty_command_tokens_fail_closed() {
        let target = host_target_triple().to_string();
        let manifest = PackageManifest::from_toml_str(&format!(
            r#"
name = "demo"
version = "1.0.0"

[[artifacts]]
target = "{target}"
url = "https://example.test/demo-1.0.0.tar.zst"
sha256 = "abc123"

[source_build]
url = "https://example.test/demo-1.0.0-src.zip"
archive_sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
build_system = "shell"
build_commands = ["", "-c", "true"]
install_commands = ["sh", "-c", "true"]
"#
        ))
        .expect("manifest should parse before source-build plan validation");

        let err = select_install_plan_for_target(&manifest, &target, true)
            .expect_err("empty source-build command tokens must fail closed");
        assert!(
            err.to_string().contains("command tokens must not be empty"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn source_build_metadata_rejects_unsupported_archive_type_fail_closed() {
        let target = host_target_triple().to_string();
        let manifest = PackageManifest::from_toml_str(&format!(
            r#"
name = "demo"
version = "1.0.0"

[[artifacts]]
target = "{target}"
url = "https://example.test/demo-1.0.0.tar.zst"
sha256 = "abc123"

[source_build]
url = "https://example.test/demo-1.0.0-src.pkg"
archive_sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
build_system = "shell"
build_commands = ["sh", "-c", "true"]
install_commands = ["sh", "-c", "true"]
"#
        ))
        .expect("manifest should parse before source-build plan validation");

        let err = select_install_plan_for_target(&manifest, &target, true)
            .expect_err("unsupported source-build archive types must fail closed");
        assert!(
            err.to_string()
                .contains("archive type 'pkg' is not supported for source builds"),
            "unexpected error: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn install_build_from_source_cli_flow_records_source_build_journal_steps() {
        let home_root = build_test_layout_path(current_unix_nanos());
        std::fs::create_dir_all(&home_root).expect("must create test HOME root");

        with_test_home_layout(&home_root, |layout| {
            layout.ensure_base_dirs().expect("must create base dirs");
            configure_ready_source(layout, "official");
            let target = host_target_triple().to_string();
            let source_archive_sha256 =
                seed_source_build_tar_gz_cache(layout, "demo", "1.0.0", &target);
            write_signed_source_build_manifest(
                layout,
                "official",
                "demo",
                "1.0.0",
                &target,
                &source_archive_sha256,
            );

            let cli = Cli::try_parse_from(["crosspack", "install", "demo", "--build-from-source"])
                .expect("install command must parse");
            run_cli(cli).expect("install --build-from-source should succeed");

            let receipts = read_install_receipts(layout).expect("must read receipts");
            assert_eq!(receipts.len(), 1, "exactly one package should be installed");
            let receipt = &receipts[0];
            assert_eq!(receipt.name, "demo");
            assert_eq!(receipt.version, "1.0.0");
            assert_eq!(
                receipt.artifact_url.as_deref(),
                Some("https://example.test/demo-1.0.0-src.tar.gz")
            );
            assert_eq!(
                receipt.artifact_sha256.as_deref(),
                Some(source_archive_sha256.as_str()),
                "source-build installs must persist source archive checksum provenance"
            );

            let txid = single_transaction_txid(layout);
            let metadata = read_transaction_metadata(layout, &txid)
                .expect("must read transaction metadata")
                .expect("metadata should exist");
            assert_eq!(metadata.operation, "install");
            assert_eq!(metadata.status, "committed");

            let records =
                read_transaction_journal_records(layout, &txid).expect("must read journal records");
            let fetch_index = records
                .iter()
                .position(|entry| entry.step == "source_fetch:demo")
                .expect("source fetch step must be journaled");
            let build_system_index = records
                .iter()
                .position(|entry| entry.step == "source_build_system:demo:shell")
                .expect("source build system step must be journaled");
            let install_index = records
                .iter()
                .position(|entry| entry.step == "source_install:demo")
                .expect("source install step must be journaled");
            assert!(
                fetch_index < build_system_index && build_system_index < install_index,
                "source-build journal step order must remain deterministic"
            );
        });

        let _ = std::fs::remove_dir_all(home_root);
    }

    #[cfg(unix)]
    #[test]
    fn upgrade_all_roots_journals_source_install_before_apply_done_task_8_inventory_gap() {
        let home_root = build_test_layout_path(current_unix_nanos());
        std::fs::create_dir_all(&home_root).expect("must create test HOME root");

        with_test_home_layout(&home_root, |layout| {
            layout.ensure_base_dirs().expect("must create base dirs");
            configure_ready_source(layout, "official");
            let target = host_target_triple().to_string();

            for package_name in ["demo", "tool"] {
                let mut receipt = install_receipt(package_name, "1.0.0", InstallReason::Root, &[]);
                receipt.target = Some(target.clone());
                std::fs::create_dir_all(layout.package_dir(package_name, "1.0.0"))
                    .expect("must create old package dir");
                write_install_receipt(layout, &receipt).expect("must write old receipt");

                let source_archive_sha256 =
                    seed_source_build_tar_gz_cache(layout, package_name, "2.0.0", &target);
                write_signed_source_build_manifest(
                    layout,
                    "official",
                    package_name,
                    "2.0.0",
                    &target,
                    &source_archive_sha256,
                );
            }

            run_upgrade_command(
                layout,
                None,
                None,
                UpgradeCommandOptions {
                    dry_run: false,
                    explain: false,
                    build_from_source: true,
                    provider_overrides: &BTreeMap::new(),
                    interaction_policy: InstallInteractionPolicy::default(),
                },
            )
            .expect("upgrade all roots should succeed");

            let txid = single_transaction_txid(layout);
            let records =
                read_transaction_journal_records(layout, &txid).expect("must read journal records");
            let steps = records
                .iter()
                .map(|entry| entry.step.as_str())
                .collect::<Vec<_>>();

            for package_name in ["demo", "tool"] {
                let source_install_index = steps
                    .iter()
                    .position(|step| *step == format!("source_install:{package_name}"))
                    .expect("source install step must be journaled");
                let apply_done_index = steps
                    .iter()
                    .position(|step| *step == format!("upgrade_package:{package_name}"))
                    .expect("upgrade apply done step must be journaled");
                assert!(
                    source_install_index < apply_done_index,
                    "Task 8 inventory gap: forward source mutation must be journaled before upgrade apply done; steps={steps:?}"
                );
            }
        });

        let _ = std::fs::remove_dir_all(home_root);
    }

    #[cfg(unix)]
    #[test]
    fn bundle_apply_build_from_source_executes_install_and_records_source_steps() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        configure_ready_source(&layout, "official");
        let target = host_target_triple().to_string();
        let source_archive_sha256 =
            seed_source_build_tar_gz_cache(&layout, "bundle-demo", "1.0.0", &target);
        write_signed_source_build_manifest(
            &layout,
            "official",
            "bundle-demo",
            "1.0.0",
            &target,
            &source_archive_sha256,
        );

        let bundle_path = layout.prefix().join("bundle-source-build.toml");
        std::fs::write(
            &bundle_path,
            r#"
format = "crosspack.bundle"
version = 1

[[roots]]
name = "bundle-demo"
"#,
        )
        .expect("must write bundle apply fixture");

        let provider_values: Vec<String> = Vec::new();
        run_bundle_apply_command(
            &layout,
            None,
            BundleApplyOptions {
                file: Some(bundle_path.as_path()),
                dry_run: false,
                explain: false,
                build_from_source: true,
                force_redownload: false,
                provider_values: &provider_values,
            },
        )
        .expect("bundle apply --build-from-source should execute install path");

        let receipts = read_install_receipts(&layout).expect("must read receipts");
        assert!(
            receipts.iter().any(|receipt| receipt.name == "bundle-demo"),
            "bundle apply should install bundle root package"
        );

        let txid = single_transaction_txid(&layout);
        let metadata = read_transaction_metadata(&layout, &txid)
            .expect("must read transaction metadata")
            .expect("metadata should exist");
        assert_eq!(metadata.operation, "bundle-apply");
        assert_eq!(metadata.status, "committed");

        let records = read_transaction_journal_records(&layout, &txid)
            .expect("must read bundle apply journal records");
        let steps = records
            .iter()
            .map(|entry| entry.step.as_str())
            .collect::<Vec<_>>();
        let source_fetch_index = steps
            .iter()
            .position(|step| *step == "source_fetch:bundle-demo")
            .expect("bundle apply source-build flow must record source_fetch step");
        let build_system_index = steps
            .iter()
            .position(|step| *step == "source_build_system:bundle-demo:shell")
            .expect("bundle apply source-build flow must record build-system step");
        let source_install_index = steps
            .iter()
            .position(|step| *step == "source_install:bundle-demo")
            .expect("bundle apply source-build flow must record source_install step");
        let apply_done_index = steps
            .iter()
            .position(|step| *step == "install_package:bundle-demo")
            .expect("bundle apply flow must record install apply done step");
        assert!(
            source_fetch_index < build_system_index
                && build_system_index < source_install_index
                && source_install_index < apply_done_index,
            "Task 8 inventory gap: bundle apply source-build mutation must be journaled before apply done; steps={steps:?}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[cfg(unix)]
    #[test]
    fn install_build_from_source_fails_closed_on_source_archive_checksum_mismatch() {
        let home_root = build_test_layout_path(current_unix_nanos());
        std::fs::create_dir_all(&home_root).expect("must create test HOME root");

        with_test_home_layout(&home_root, |layout| {
            layout.ensure_base_dirs().expect("must create base dirs");
            configure_ready_source(layout, "official");
            let target = host_target_triple().to_string();
            let _source_archive_sha256 =
                seed_source_build_tar_gz_cache(layout, "checksum-demo", "1.0.0", &target);
            write_signed_source_build_manifest(
                layout,
                "official",
                "checksum-demo",
                "1.0.0",
                &target,
                EMPTY_SHA256,
            );

            let cli = Cli::try_parse_from([
                "crosspack",
                "install",
                "checksum-demo",
                "--build-from-source",
            ])
            .expect("install command must parse");
            let err = run_cli(cli).expect_err("checksum mismatch must fail closed");
            assert!(
                err.to_string().contains("source archive sha256 mismatch"),
                "unexpected error: {err}"
            );

            let receipts = read_install_receipts(layout).expect("must read receipts after failure");
            assert!(
                receipts
                    .iter()
                    .all(|receipt| receipt.name != "checksum-demo"),
                "checksum mismatch must not persist an install receipt"
            );
        });

        let _ = std::fs::remove_dir_all(home_root);
    }

    #[cfg(unix)]
    #[test]
    fn failed_source_build_does_not_mark_build_system_journal_step_done() {
        let home_root = build_test_layout_path(current_unix_nanos());
        std::fs::create_dir_all(&home_root).expect("must create test HOME root");

        with_test_home_layout(&home_root, |layout| {
            layout.ensure_base_dirs().expect("must create base dirs");
            configure_ready_source(layout, "official");
            let target = host_target_triple().to_string();
            let source_archive_sha256 =
                seed_source_build_tar_gz_cache(layout, "journal-demo", "1.0.0", &target);
            write_signed_source_build_manifest_with_commands(
                layout,
                "official",
                "journal-demo",
                "1.0.0",
                &target,
                &source_archive_sha256,
                SourceBuildScripts {
                    build: "exit 7".to_string(),
                    install: "true".to_string(),
                },
            );

            let cli = Cli::try_parse_from([
                "crosspack",
                "install",
                "journal-demo",
                "--build-from-source",
            ])
            .expect("install command must parse");
            let err = run_cli(cli).expect_err("build failure must fail install");
            assert!(
                err.to_string()
                    .contains("source build build command failed"),
                "unexpected error: {err}"
            );

            let txid = single_transaction_txid(layout);
            let records = read_transaction_journal_records(layout, &txid)
                .expect("must read source-build failure journal records");
            assert!(
                records
                    .iter()
                    .any(|entry| entry.step == "source_fetch:journal-demo"),
                "source fetch should still be journaled after successful download"
            );
            assert!(
                records
                    .iter()
                    .all(|entry| entry.step != "source_build_system:journal-demo:shell"),
                "source build system step must not be journaled as done before successful build"
            );
            assert!(
                records
                    .iter()
                    .all(|entry| entry.step != "source_install:journal-demo"),
                "source install step must not be journaled when build fails"
            );
        });

        let _ = std::fs::remove_dir_all(home_root);
    }

    #[test]
    fn install_build_from_source_dry_run_keeps_machine_contract_keys_and_order() {
        let target = host_target_triple().to_string();
        let manifest = PackageManifest::from_toml_str(&format!(
            r#"
name = "dryrun-demo"
version = "1.0.0"

[[artifacts]]
target = "{target}"
url = "https://example.test/dryrun-demo-1.0.0.tar.zst"
sha256 = "abc123"

[source_build]
url = "https://example.test/dryrun-demo-1.0.0-src.zip"
archive_sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
build_system = "shell"
build_commands = ["sh", "-c", "true"]
install_commands = ["sh", "-c", "true"]
"#
        ))
        .expect("manifest should parse");

        let (binary_artifact, _) = select_install_plan_for_target(&manifest, &target, false)
            .expect("binary path must resolve");
        let binary_planned = build_planned_package_changes(
            &[ResolvedInstall {
                manifest: manifest.clone(),
                artifact: binary_artifact.clone(),
                resolved_target: target.clone(),
                archive_type: binary_artifact
                    .archive_type()
                    .expect("artifact archive type must parse"),
                source_build: None,
            }],
            &[],
        )
        .expect("binary planned changes must build");
        let binary_lines = render_dry_run_output_lines(
            &build_transaction_preview("install", &binary_planned),
            TransactionPreviewMode::DryRun,
            None,
        );

        let (source_artifact, source_build) =
            select_install_plan_for_target(&manifest, &target, true)
                .expect("source-build path must resolve");
        let source_planned = build_planned_package_changes(
            &[ResolvedInstall {
                manifest,
                artifact: source_artifact,
                resolved_target: target,
                archive_type: ArchiveType::Zip,
                source_build,
            }],
            &[],
        )
        .expect("source-build planned changes must build");
        let source_lines = render_dry_run_output_lines(
            &build_transaction_preview("install", &source_planned),
            TransactionPreviewMode::DryRun,
            None,
        );

        assert_eq!(source_lines, binary_lines);
        assert!(source_lines[0].starts_with("transaction_preview "));
        assert!(source_lines[1].starts_with("transaction_summary "));
        assert!(source_lines[2].starts_with("risk_flags="));
        assert!(source_lines[3].starts_with("change_add "));
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
            source_build: None,
        }
    }

    fn install_receipt(
        name: &str,
        version: &str,
        install_reason: InstallReason,
        dependencies: &[&str],
    ) -> InstallReceipt {
        InstallReceipt {
            name: name.to_string(),
            version: version.to_string(),
            dependencies: dependencies.iter().map(|dependency| dependency.to_string()).collect(),
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: Vec::new(),
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
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
            exposed_integrations: Vec::new(),
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

    fn assert_terminal_snapshot(name: &str, output: String) {
        insta::with_settings!({ prepend_module_to_snapshot => false }, {
            insta::assert_snapshot!(name, output);
        });
    }

    fn empty_update_report() -> super::UpdateReport {
        super::UpdateReport {
            lines: Vec::new(),
            updated: 0,
            up_to_date: 0,
            failed: 0,
        }
    }

    static TEST_LAYOUT_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[cfg(unix)]
    fn home_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn download_backend_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn ui_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvVarGuard {
        name: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(name: &'static str, value: &str) -> Self {
            let previous = std::env::var(name).ok();
            unsafe {
                std::env::set_var(name, value);
            }
            Self { name, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.previous.as_deref() {
                Some(value) => unsafe {
                    std::env::set_var(self.name, value);
                },
                None => unsafe {
                    std::env::remove_var(self.name);
                },
            }
        }
    }

    struct DownloadBackendEnvGuard {
        previous: Option<String>,
    }

    impl DownloadBackendEnvGuard {
        fn set(value: &str) -> Self {
            let previous = std::env::var("CROSSPACK_DOWNLOAD_BACKEND").ok();
            unsafe {
                std::env::set_var("CROSSPACK_DOWNLOAD_BACKEND", value);
            }
            Self { previous }
        }
    }

    impl Drop for DownloadBackendEnvGuard {
        fn drop(&mut self) {
            match self.previous.as_deref() {
                Some(value) => unsafe {
                    std::env::set_var("CROSSPACK_DOWNLOAD_BACKEND", value);
                },
                None => unsafe {
                    std::env::remove_var("CROSSPACK_DOWNLOAD_BACKEND");
                },
            }
        }
    }

    #[cfg(unix)]
    struct HomeEnvGuard {
        previous: Option<String>,
    }

    #[cfg(unix)]
    impl HomeEnvGuard {
        fn set(home: &Path) -> Self {
            let previous = std::env::var("HOME").ok();
            unsafe {
                std::env::set_var("HOME", home);
            }
            Self { previous }
        }
    }

    #[cfg(unix)]
    impl Drop for HomeEnvGuard {
        fn drop(&mut self) {
            match self.previous.as_deref() {
                Some(value) => unsafe {
                    std::env::set_var("HOME", value);
                },
                None => unsafe {
                    std::env::remove_var("HOME");
                },
            }
        }
    }

    fn current_unix_nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    }

    #[cfg(unix)]
    fn with_test_home_layout<T>(home_root: &Path, run: impl FnOnce(&PrefixLayout) -> T) -> T {
        let _home_lock = home_env_lock()
            .lock()
            .expect("HOME env lock should not be poisoned");
        let _home_guard = HomeEnvGuard::set(home_root);
        let layout = PrefixLayout::new(home_root.join(".crosspack"));
        run(&layout)
    }

    fn single_transaction_txid(layout: &PrefixLayout) -> String {
        let mut txids = std::fs::read_dir(layout.transactions_dir())
            .expect("must read transactions dir")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .filter_map(|path| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(ToOwned::to_owned)
            })
            .collect::<Vec<_>>();
        txids.sort();
        assert_eq!(
            txids.len(),
            1,
            "expected exactly one transaction metadata file, found {:?}",
            txids
        );
        txids.remove(0)
    }

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

    struct TestLayoutCleanup {
        prefix: PathBuf,
    }

    impl TestLayoutCleanup {
        fn new(layout: &PrefixLayout) -> Self {
            Self {
                prefix: layout.prefix().to_path_buf(),
            }
        }
    }

    impl Drop for TestLayoutCleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.prefix);
        }
    }

    fn test_registry_source_dir(name: &str, with_registry_pub: bool) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = current_unix_nanos();
        path.push(format!("crosspack-cli-test-registry-{name}-{nanos}"));
        std::fs::create_dir_all(path.join("packages")).expect("must create packages dir");
        std::fs::create_dir_all(path.join("releases")).expect("must create releases dir");
        if with_registry_pub {
            std::fs::write(path.join("registry.pub"), "test-key\n")
                .expect("must write registry key");
        }
        path
    }

    fn configure_ready_source(layout: &PrefixLayout, source_name: &str) {
        let state_root = registry_state_root(layout);
        configure_ready_cache_source(layout, source_name);
        std::fs::write(
            state_root.join("sources.toml"),
            format!(
                "version = 1\n\n[[sources]]\nname = \"{source_name}\"\nkind = \"filesystem\"\nlocation = \"/tmp/{source_name}\"\nfingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\nenabled = true\npriority = 1\n"
            ),
        )
        .expect("must write source state");
    }

    fn configure_ready_cache_source(layout: &PrefixLayout, source_name: &str) {
        let state_root = registry_state_root(layout);
        std::fs::create_dir_all(state_root.join("cache").join(source_name))
            .expect("must create source cache root");
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
        write_signed_test_manifest_with_targets(
            layout,
            TestManifestSpec {
                source_name,
                package_name,
                version,
                license,
                homepage,
                provides,
                targets: &["x86_64-unknown-linux-gnu"],
            },
        );
    }

    struct TestManifestSpec<'a> {
        source_name: &'a str,
        package_name: &'a str,
        version: &'a str,
        license: Option<&'a str>,
        homepage: Option<&'a str>,
        provides: &'a [&'a str],
        targets: &'a [&'a str],
    }

    fn write_signed_test_manifest_with_targets(layout: &PrefixLayout, spec: TestManifestSpec<'_>) {
        let cache_root = registry_state_root(layout)
            .join("cache")
            .join(spec.source_name);
        let package_template_path = cache_root
            .join("packages")
            .join(format!("{}.toml", spec.package_name));
        let package_dir = cache_root.join("releases").join(spec.package_name);
        std::fs::create_dir_all(&package_dir).expect("must create package directory");
        std::fs::create_dir_all(cache_root.join("packages"))
            .expect("must create package template directory");

        let signing_key = test_signing_key();
        std::fs::write(
            cache_root.join("registry.pub"),
            public_key_hex(&signing_key),
        )
        .expect("must write registry key");
        let package_template = format!("name = \"{}\"\n", spec.package_name);
        std::fs::write(&package_template_path, package_template.as_bytes())
            .expect("must write package template");
        let package_signature = signing_key.sign(package_template.as_bytes());
        std::fs::write(
            package_template_path.with_extension("toml.sig"),
            hex::encode(package_signature.to_bytes()),
        )
        .expect("must write package template signature");

        let manifest = manifest_toml(
            spec.package_name,
            spec.version,
            spec.license,
            spec.homepage,
            spec.provides,
            spec.targets,
        );
        let manifest_path = package_dir.join(format!("{}.toml", spec.version));
        std::fs::write(&manifest_path, manifest.as_bytes()).expect("must write manifest");

        let signature = signing_key.sign(manifest.as_bytes());
        std::fs::write(
            manifest_path.with_extension("toml.sig"),
            hex::encode(signature.to_bytes()),
        )
        .expect("must write signature");
    }

    fn write_signed_policy_manifest(
        layout: &PrefixLayout,
        source_name: &str,
        package_name: &str,
        manifest: &str,
    ) {
        let parsed = PackageManifest::from_toml_str(manifest).expect("policy manifest must parse");
        assert_eq!(
            parsed.name, package_name,
            "test package name must match manifest name"
        );
        let cache_root = registry_state_root(layout).join("cache").join(source_name);
        let package_template_path = cache_root.join("packages").join(format!("{package_name}.toml"));
        let package_dir = cache_root.join("releases").join(package_name);
        std::fs::create_dir_all(&package_dir).expect("must create package directory");
        std::fs::create_dir_all(cache_root.join("packages"))
            .expect("must create package template directory");

        let signing_key = test_signing_key();
        std::fs::write(cache_root.join("registry.pub"), public_key_hex(&signing_key))
            .expect("must write registry key");
        let package_template = format!("name = \"{package_name}\"\n");
        std::fs::write(&package_template_path, package_template.as_bytes())
            .expect("must write package template");
        let package_signature = signing_key.sign(package_template.as_bytes());
        std::fs::write(
            package_template_path.with_extension("toml.sig"),
            hex::encode(package_signature.to_bytes()),
        )
        .expect("must write package template signature");

        let manifest_path = package_dir.join(format!("{}.toml", parsed.version));
        std::fs::write(&manifest_path, manifest.as_bytes()).expect("must write manifest");
        let signature = signing_key.sign(manifest.as_bytes());
        std::fs::write(
            manifest_path.with_extension("toml.sig"),
            hex::encode(signature.to_bytes()),
        )
        .expect("must write signature");
    }

    fn write_invalid_policy_manifest(
        layout: &PrefixLayout,
        source_name: &str,
        package_name: &str,
        manifest: &str,
    ) {
        write_signed_policy_manifest(layout, source_name, package_name, manifest);
        let parsed = PackageManifest::from_toml_str(manifest).expect("policy manifest must parse");
        let sig_path = registry_state_root(layout)
            .join("cache")
            .join(source_name)
            .join("releases")
            .join(package_name)
            .join(format!("{}.toml.sig", parsed.version));
        std::fs::write(sig_path, "00").expect("must corrupt manifest signature");
    }

    fn write_signed_source_build_metadata_manifest(
        layout: &PrefixLayout,
        source_name: &str,
        package_name: &str,
        version: &str,
        artifact_target: &str,
    ) {
        let cache_root = registry_state_root(layout).join("cache").join(source_name);
        let package_template_path = cache_root
            .join("packages")
            .join(format!("{package_name}.toml"));
        let package_dir = cache_root.join("releases").join(package_name);
        std::fs::create_dir_all(&package_dir).expect("must create package directory");
        std::fs::create_dir_all(cache_root.join("packages"))
            .expect("must create package template directory");

        let signing_key = test_signing_key();
        std::fs::write(
            cache_root.join("registry.pub"),
            public_key_hex(&signing_key),
        )
        .expect("must write registry key");
        let package_template = format!("name = \"{package_name}\"\n");
        std::fs::write(&package_template_path, package_template.as_bytes())
            .expect("must write package template");
        let package_signature = signing_key.sign(package_template.as_bytes());
        std::fs::write(
            package_template_path.with_extension("toml.sig"),
            hex::encode(package_signature.to_bytes()),
        )
        .expect("must write package template signature");

        let manifest = format!(
            r#"
name = "{package_name}"
version = "{version}"

[[artifacts]]
target = "{artifact_target}"
url = "https://example.test/{package_name}-{version}-{artifact_target}.tar.zst"
sha256 = "abc123"

[source_build]
url = "https://example.test/{package_name}-{version}-src.tar.gz"
archive_sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
build_system = "cargo"
build_commands = ["cargo", "build", "--release"]
install_commands = ["cargo", "install", "--path", "."]
"#
        );
        let manifest_path = package_dir.join(format!("{version}.toml"));
        std::fs::write(&manifest_path, manifest.as_bytes()).expect("must write manifest");

        let signature = signing_key.sign(manifest.as_bytes());
        std::fs::write(
            manifest_path.with_extension("toml.sig"),
            hex::encode(signature.to_bytes()),
        )
        .expect("must write signature");
    }

    #[cfg(unix)]
    fn write_signed_source_build_manifest(
        layout: &PrefixLayout,
        source_name: &str,
        package_name: &str,
        version: &str,
        target: &str,
        source_archive_sha256: &str,
    ) {
        write_signed_source_build_manifest_with_commands(
            layout,
            source_name,
            package_name,
            version,
            target,
            source_archive_sha256,
            SourceBuildScripts {
                build: "mkdir -p $CROSSPACK_STAGE_DIR/bin".to_string(),
                install: format!(
                    "echo '#!/bin/sh' > $CROSSPACK_STAGE_DIR/bin/{package_name}; echo 'exit 0' >> $CROSSPACK_STAGE_DIR/bin/{package_name}; chmod +x $CROSSPACK_STAGE_DIR/bin/{package_name}"
                ),
            },
        );
    }

    #[cfg(unix)]
    struct SourceBuildScripts {
        build: String,
        install: String,
    }

    #[cfg(unix)]
    fn write_signed_source_build_manifest_with_commands(
        layout: &PrefixLayout,
        source_name: &str,
        package_name: &str,
        version: &str,
        target: &str,
        source_archive_sha256: &str,
        scripts: SourceBuildScripts,
    ) {
        let manifest = format!(
            r#"
name = "{package_name}"
version = "{version}"

[[artifacts]]
target = "{target}"
url = "https://example.test/{package_name}-{version}.tar.zst"
sha256 = "abc123"
[[artifacts.binaries]]
name = "{package_name}"
path = "bin/{package_name}"

[source_build]
url = "https://example.test/{package_name}-{version}-src.tar.gz"
archive_sha256 = "{source_archive_sha256}"
build_system = "shell"
build_commands = ["sh", "-c", "{build_script}"]
install_commands = ["sh", "-c", "{install_script}"]
"#,
            build_script = scripts.build,
            install_script = scripts.install
        );

        let cache_root = registry_state_root(layout).join("cache").join(source_name);
        let package_template_path = cache_root
            .join("packages")
            .join(format!("{package_name}.toml"));
        let package_dir = cache_root.join("releases").join(package_name);
        std::fs::create_dir_all(&package_dir).expect("must create package directory");
        std::fs::create_dir_all(cache_root.join("packages"))
            .expect("must create package template directory");

        let signing_key = test_signing_key();
        std::fs::write(
            cache_root.join("registry.pub"),
            public_key_hex(&signing_key),
        )
        .expect("must write registry key");
        let package_template = format!("name = \"{package_name}\"\n");
        std::fs::write(&package_template_path, package_template.as_bytes())
            .expect("must write package template");
        let package_signature = signing_key.sign(package_template.as_bytes());
        std::fs::write(
            package_template_path.with_extension("toml.sig"),
            hex::encode(package_signature.to_bytes()),
        )
        .expect("must write package template signature");

        let manifest_path = package_dir.join(format!("{version}.toml"));
        std::fs::write(&manifest_path, manifest.as_bytes()).expect("must write manifest");

        let signature = signing_key.sign(manifest.as_bytes());
        std::fs::write(
            manifest_path.with_extension("toml.sig"),
            hex::encode(signature.to_bytes()),
        )
        .expect("must write signature");
    }

    #[cfg(unix)]
    fn seed_source_build_tar_gz_cache(
        layout: &PrefixLayout,
        package_name: &str,
        version: &str,
        target: &str,
    ) -> String {
        let cache_path =
            layout.artifact_cache_path(package_name, version, target, ArchiveType::TarGz);
        std::fs::create_dir_all(
            cache_path
                .parent()
                .expect("artifact cache path must have a parent"),
        )
        .expect("must create source-build cache dir");

        let fixture_root = layout
            .tmp_state_dir()
            .join(format!("source-build-archive-{package_name}-{version}"));
        let _ = std::fs::remove_dir_all(&fixture_root);
        std::fs::create_dir_all(&fixture_root).expect("must create source archive fixture root");
        std::fs::write(fixture_root.join("README.txt"), "source-build fixture\n")
            .expect("must write source archive fixture file");

        let status = std::process::Command::new("tar")
            .arg("-czf")
            .arg(&cache_path)
            .arg("-C")
            .arg(&fixture_root)
            .arg(".")
            .status()
            .expect("must spawn tar to build source archive fixture");
        assert!(
            status.success(),
            "tar must create source archive fixture successfully"
        );

        let archive_bytes = std::fs::read(&cache_path).expect("must read source archive fixture");
        let archive_sha256 = crosspack_security::sha256_hex(&archive_bytes);

        let _ = std::fs::remove_dir_all(fixture_root);
        archive_sha256
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
        targets: &[&str],
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
        for target in targets {
            manifest.push_str("[[artifacts]]\n");
            manifest.push_str(&format!("target = \"{target}\"\n"));
            manifest.push_str("url = \"https://example.test/artifact.tar.zst\"\n");
            manifest.push_str("sha256 = \"abc\"\n");
        }
        manifest
    }
}
