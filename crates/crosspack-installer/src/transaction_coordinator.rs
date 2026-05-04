use anyhow::{Context, Result};
use std::fs;
use std::io;

use crate::{
    clear_active_transaction, read_active_transaction_marker, read_transaction_journal_entries,
    read_transaction_metadata, remove_file_if_exists, set_active_transaction,
    update_transaction_status, write_transaction_metadata, ActiveTransactionMarker, PrefixLayout,
    TransactionMetadata, TransactionRecoveryAction, TransactionRepairReason, TransactionStatus,
};

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransactionBeginCrashHook {
    AfterMetadataWrite,
    AfterActiveMarker,
}

pub struct TransactionCoordinator<'a> {
    layout: &'a PrefixLayout,
}

#[derive(Debug)]
pub struct StartedTransaction {
    pub metadata: TransactionMetadata,
}

impl<'a> TransactionCoordinator<'a> {
    pub fn new(layout: &'a PrefixLayout) -> Self {
        Self { layout }
    }

    pub fn begin(
        &self,
        operation: &str,
        snapshot_id: Option<&str>,
        started_at_unix: u64,
    ) -> Result<StartedTransaction> {
        self.begin_impl(
            operation,
            snapshot_id,
            started_at_unix,
            #[cfg(test)]
            None,
        )
    }

    #[cfg(test)]
    pub(crate) fn begin_with_crash_hook_for_test(
        &self,
        operation: &str,
        snapshot_id: Option<&str>,
        started_at_unix: u64,
        crash_hook: TransactionBeginCrashHook,
    ) -> Result<StartedTransaction> {
        self.begin_impl(operation, snapshot_id, started_at_unix, Some(crash_hook))
    }

    fn begin_impl(
        &self,
        operation: &str,
        snapshot_id: Option<&str>,
        started_at_unix: u64,
        #[cfg(test)] crash_hook: Option<TransactionBeginCrashHook>,
    ) -> Result<StartedTransaction> {
        let txid = format!("tx-{started_at_unix}-{}", std::process::id());
        let metadata = TransactionMetadata {
            version: 1,
            txid,
            operation: operation.to_string(),
            status: TransactionStatus::Planning,
            started_at_unix,
            snapshot_id: snapshot_id.map(ToOwned::to_owned),
        };

        write_transaction_metadata(self.layout, &metadata)?;
        #[cfg(test)]
        if crash_hook == Some(TransactionBeginCrashHook::AfterMetadataWrite) {
            return Err(anyhow::anyhow!(
                "test crash after transaction metadata write"
            ));
        }
        if let Err(err) = set_active_transaction(self.layout, &metadata.txid) {
            let _ = remove_file_if_exists(&self.layout.transaction_metadata_path(&metadata.txid));
            let _ = std::fs::remove_dir_all(self.layout.transaction_staging_path(&metadata.txid));
            return Err(err);
        }
        #[cfg(test)]
        if crash_hook == Some(TransactionBeginCrashHook::AfterActiveMarker) {
            return Err(anyhow::anyhow!(
                "test crash after active transaction marker"
            ));
        }

        Ok(StartedTransaction { metadata })
    }

    pub fn mark_applying(&self, txid: &str) -> Result<()> {
        update_transaction_status(self.layout, txid, TransactionStatus::Applying)
    }

    pub fn mark_committed(&self, txid: &str) -> Result<()> {
        update_transaction_status(self.layout, txid, TransactionStatus::Committed)
    }

    pub fn mark_failed(&self, txid: &str) -> Result<()> {
        update_transaction_status(self.layout, txid, TransactionStatus::Failed)
    }

    pub fn mark_rolling_back(&self, txid: &str) -> Result<()> {
        update_transaction_status(self.layout, txid, TransactionStatus::RollingBack)
    }

    pub fn mark_rolled_back(&self, txid: &str) -> Result<()> {
        update_transaction_status(self.layout, txid, TransactionStatus::RolledBack)
    }

    pub fn clear_active(&self) -> Result<()> {
        clear_active_transaction(self.layout)
    }

    pub fn repair_transaction_state(&self) -> Result<TransactionRecoveryAction> {
        let action = self.classify_recovery()?;
        match &action {
            TransactionRecoveryAction::Clean
            | TransactionRecoveryAction::BlockedFailed { .. }
            | TransactionRecoveryAction::RepairRequired(_) => {}
            TransactionRecoveryAction::CleanupPlanning { txid } => {
                self.clear_active_if_points_to(txid)?;
                match fs::remove_dir_all(self.layout.transaction_staging_path(txid)) {
                    Ok(()) => {}
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                    Err(err) => {
                        return Err(err).with_context(|| {
                            format!(
                                "failed to remove transaction staging dir: {}",
                                self.layout.transaction_staging_path(txid).display()
                            )
                        });
                    }
                }
                remove_file_if_exists(&self.layout.transaction_journal_path(txid))?;
                remove_file_if_exists(&self.layout.transaction_metadata_path(txid))?;
            }
            TransactionRecoveryAction::FinalizeCommitted { txid }
            | TransactionRecoveryAction::ClearRolledBack { txid } => {
                self.clear_active_if_points_to(txid)?;
            }
            TransactionRecoveryAction::Rollback { txid }
            | TransactionRecoveryAction::ResumeRollback { txid } => {
                if !self.has_rollback_evidence(txid)? {
                    return Ok(TransactionRecoveryAction::RepairRequired(
                        TransactionRepairReason::RollbackEvidenceMissing { txid: txid.clone() },
                    ));
                }
            }
        }
        Ok(action)
    }

    pub fn classify_recovery(&self) -> Result<TransactionRecoveryAction> {
        match read_active_transaction_marker(self.layout) {
            Ok(ActiveTransactionMarker::Absent) => self.classify_recovery_without_active_marker(),
            Ok(ActiveTransactionMarker::Invalid) => Ok(TransactionRecoveryAction::RepairRequired(
                TransactionRepairReason::ActiveMarkerInvalid {
                    path: self.layout.transaction_active_path().display().to_string(),
                },
            )),
            Ok(ActiveTransactionMarker::Present(txid)) => {
                self.classify_recovery_for_active_marker(&txid)
            }
            Err(_) => Ok(TransactionRecoveryAction::RepairRequired(
                TransactionRepairReason::ActiveMarkerUnreadable,
            )),
        }
    }

    fn classify_recovery_for_active_marker(&self, txid: &str) -> Result<TransactionRecoveryAction> {
        let Some(metadata) = (match read_transaction_metadata(self.layout, txid) {
            Ok(metadata) => metadata,
            Err(_) => {
                return Ok(TransactionRecoveryAction::RepairRequired(
                    TransactionRepairReason::MetadataUnreadable {
                        txid: txid.to_string(),
                    },
                ));
            }
        }) else {
            return Ok(TransactionRecoveryAction::RepairRequired(
                TransactionRepairReason::ActiveMarkerWithoutMetadata {
                    txid: txid.to_string(),
                },
            ));
        };
        if metadata.txid != txid {
            return Ok(TransactionRecoveryAction::RepairRequired(
                TransactionRepairReason::MetadataTxidMismatch {
                    expected: txid.to_string(),
                    actual: metadata.txid,
                },
            ));
        }

        let journal_entries = match read_transaction_journal_entries(self.layout, txid) {
            Ok(entries) => entries,
            Err(_) => {
                return Ok(TransactionRecoveryAction::RepairRequired(
                    TransactionRepairReason::JournalUnreadable {
                        txid: txid.to_string(),
                    },
                ));
            }
        };

        self.classify_metadata_with_journal_evidence(&metadata, !journal_entries.is_empty())
    }

    fn classify_metadata_with_journal_evidence(
        &self,
        metadata: &TransactionMetadata,
        has_journal_entries: bool,
    ) -> Result<TransactionRecoveryAction> {
        let txid = metadata.txid.clone();
        let action = match metadata.status {
            TransactionStatus::Planning => {
                if has_journal_entries || self.transaction_staging_has_payload(&metadata.txid)? {
                    TransactionRecoveryAction::Rollback { txid }
                } else {
                    TransactionRecoveryAction::CleanupPlanning { txid }
                }
            }
            TransactionStatus::Applying => TransactionRecoveryAction::Rollback { txid },
            TransactionStatus::Completed | TransactionStatus::Committed => {
                TransactionRecoveryAction::FinalizeCommitted { txid }
            }
            TransactionStatus::RollingBack => TransactionRecoveryAction::ResumeRollback { txid },
            TransactionStatus::RolledBack => TransactionRecoveryAction::ClearRolledBack { txid },
            TransactionStatus::Failed => TransactionRecoveryAction::BlockedFailed { txid },
        };
        Ok(action)
    }

    fn classify_recovery_without_active_marker(&self) -> Result<TransactionRecoveryAction> {
        for txid in self.discover_transaction_metadata_ids()? {
            let metadata = match read_transaction_metadata(self.layout, &txid) {
                Ok(Some(metadata)) => metadata,
                Ok(None) => continue,
                Err(_) => {
                    return Ok(TransactionRecoveryAction::RepairRequired(
                        TransactionRepairReason::MetadataUnreadable { txid },
                    ));
                }
            };
            if metadata.txid != txid {
                return Ok(TransactionRecoveryAction::RepairRequired(
                    TransactionRepairReason::MetadataTxidMismatch {
                        expected: txid,
                        actual: metadata.txid,
                    },
                ));
            }

            let journal_entries = match read_transaction_journal_entries(self.layout, &txid) {
                Ok(entries) => entries,
                Err(_err) => {
                    return Ok(TransactionRecoveryAction::RepairRequired(
                        TransactionRepairReason::JournalUnreadable { txid },
                    ));
                }
            };

            let txid = metadata.txid.clone();
            match metadata.status {
                TransactionStatus::Planning => {
                    return self.classify_metadata_with_journal_evidence(
                        &metadata,
                        !journal_entries.is_empty(),
                    );
                }
                TransactionStatus::Applying => {
                    return Ok(TransactionRecoveryAction::RepairRequired(
                        TransactionRepairReason::ApplyingWithoutActiveMarker { txid },
                    ));
                }
                TransactionStatus::RollingBack => {
                    return Ok(TransactionRecoveryAction::ResumeRollback { txid });
                }
                TransactionStatus::Failed => {
                    return Ok(TransactionRecoveryAction::BlockedFailed { txid });
                }
                TransactionStatus::Completed
                | TransactionStatus::Committed
                | TransactionStatus::RolledBack => {}
            }
        }

        Ok(TransactionRecoveryAction::Clean)
    }

    fn discover_transaction_metadata_ids(&self) -> Result<Vec<String>> {
        let entries = match fs::read_dir(self.layout.transactions_dir()) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "failed to list transaction metadata: {}",
                        self.layout.transactions_dir().display()
                    )
                });
            }
        };

        let mut txids = Vec::new();
        for entry in entries {
            let entry = entry.with_context(|| {
                format!(
                    "failed to list transaction metadata: {}",
                    self.layout.transactions_dir().display()
                )
            })?;
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            if let Some(txid) = path.file_stem().and_then(|stem| stem.to_str()) {
                txids.push(txid.to_string());
            }
        }
        txids.sort();
        Ok(txids)
    }

    fn transaction_staging_has_payload(&self, txid: &str) -> Result<bool> {
        let staging_path = self.layout.transaction_staging_path(txid);
        let mut entries = match fs::read_dir(&staging_path) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "failed to list transaction staging dir: {}",
                        staging_path.display()
                    )
                });
            }
        };

        Ok(entries.next().transpose()?.is_some())
    }

    fn clear_active_if_points_to(&self, txid: &str) -> Result<()> {
        if read_active_transaction_marker(self.layout)?
            == ActiveTransactionMarker::Present(txid.to_string())
        {
            clear_active_transaction(self.layout)?;
        }
        Ok(())
    }

    fn has_rollback_evidence(&self, txid: &str) -> Result<bool> {
        let journal_entries = read_transaction_journal_entries(self.layout, txid)?;
        Ok(!journal_entries.is_empty() || self.transaction_staging_has_payload(txid)?)
    }
}
