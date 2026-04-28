use anyhow::Result;

use crate::{
    clear_active_transaction, remove_file_if_exists, set_active_transaction,
    update_transaction_status, write_transaction_metadata, PrefixLayout, TransactionMetadata,
    TransactionStatus,
};

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
        if let Err(err) = set_active_transaction(self.layout, &metadata.txid) {
            let _ = remove_file_if_exists(&self.layout.transaction_metadata_path(&metadata.txid));
            let _ = std::fs::remove_dir_all(self.layout.transaction_staging_path(&metadata.txid));
            return Err(err);
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
}
