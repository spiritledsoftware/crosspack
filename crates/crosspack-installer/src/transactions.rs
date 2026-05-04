use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::durable;
use crate::{PrefixLayout, TransactionJournalEntry, TransactionMetadata, TransactionStatus};

#[cfg(test)]
static FAIL_ACTIVE_TRANSACTION_AFTER_WRITE_FOR_TEST: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
pub(crate) fn fail_next_active_transaction_after_write_for_test() {
    FAIL_ACTIVE_TRANSACTION_AFTER_WRITE_FOR_TEST.store(true, Ordering::SeqCst);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveTransactionMarker {
    Absent,
    Invalid,
    Present(String),
}

pub fn set_active_transaction(layout: &PrefixLayout, txid: &str) -> Result<PathBuf> {
    let path = layout.transaction_active_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut file = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            let existing = read_active_transaction(layout).ok().flatten();
            let detail = existing
                .map(|existing_txid| format!(" (txid={existing_txid})"))
                .unwrap_or_default();
            return Err(anyhow!("active transaction marker already exists{detail}"));
        }
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "failed to claim active transaction file: {}",
                    path.display()
                )
            });
        }
    };

    let write_result = (|| -> Result<()> {
        file.write_all(format!("{txid}\n").as_bytes())
            .with_context(|| {
                format!(
                    "failed to write active transaction file: {}",
                    path.display()
                )
            })?;
        #[cfg(test)]
        if FAIL_ACTIVE_TRANSACTION_AFTER_WRITE_FOR_TEST.swap(false, Ordering::SeqCst) {
            anyhow::bail!("test active transaction failure after write");
        }
        file.sync_all().with_context(|| {
            format!(
                "failed to flush active transaction file: {}",
                path.display()
            )
        })?;
        if let Some(parent) = path.parent() {
            durable::sync_directory(parent)?;
        }
        Ok(())
    })();
    drop(file);

    if write_result.is_err() {
        let _ = durable::remove_file_if_exists_durable(&path);
    }
    write_result?;

    Ok(path)
}

pub fn read_active_transaction(layout: &PrefixLayout) -> Result<Option<String>> {
    let path = layout.transaction_active_path();
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("failed to read active transaction file: {}", path.display())
            });
        }
    };

    let txid = raw.trim();
    if txid.is_empty() {
        return Ok(None);
    }

    Ok(Some(txid.to_string()))
}

pub fn read_active_transaction_marker(layout: &PrefixLayout) -> Result<ActiveTransactionMarker> {
    let path = layout.transaction_active_path();
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Ok(ActiveTransactionMarker::Absent);
        }
        Err(err) => {
            return Err(err).with_context(|| {
                format!("failed to read active transaction file: {}", path.display())
            });
        }
    };

    let txid = raw.trim();
    if !is_valid_active_txid(txid) {
        return Ok(ActiveTransactionMarker::Invalid);
    }

    Ok(ActiveTransactionMarker::Present(txid.to_string()))
}

fn is_valid_active_txid(txid: &str) -> bool {
    !txid.is_empty()
        && txid.starts_with("tx-")
        && txid.len() <= 128
        && txid
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub fn clear_active_transaction(layout: &PrefixLayout) -> Result<()> {
    let path = layout.transaction_active_path();
    durable::remove_file_if_exists_durable(&path).with_context(|| {
        format!(
            "failed to clear active transaction file: {}",
            path.display()
        )
    })?;
    Ok(())
}

pub fn write_transaction_metadata(
    layout: &PrefixLayout,
    metadata: &TransactionMetadata,
) -> Result<PathBuf> {
    let path = layout.transaction_metadata_path(&metadata.txid);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::create_dir_all(layout.transaction_staging_path(&metadata.txid)).with_context(|| {
        format!(
            "failed to create transaction staging dir: {}",
            layout.transaction_staging_path(&metadata.txid).display()
        )
    })?;

    durable::write_file_atomic(&path, serialize_transaction_metadata(metadata).as_bytes())
        .with_context(|| {
            format!(
                "failed to write transaction metadata file: {}",
                path.display()
            )
        })?;
    Ok(path)
}

pub fn read_transaction_metadata(
    layout: &PrefixLayout,
    txid: &str,
) -> Result<Option<TransactionMetadata>> {
    let path = layout.transaction_metadata_path(txid);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "failed to read transaction metadata file: {}",
                    path.display()
                )
            });
        }
    };

    let metadata = parse_transaction_metadata(&raw).with_context(|| {
        format!(
            "failed parsing transaction metadata file: {}",
            path.display()
        )
    })?;
    Ok(Some(metadata))
}

pub fn update_transaction_status(
    layout: &PrefixLayout,
    txid: &str,
    status: TransactionStatus,
) -> Result<()> {
    let mut metadata = read_transaction_metadata(layout, txid)?
        .ok_or_else(|| anyhow!("transaction metadata not found for '{txid}'"))?;
    if metadata.txid != txid {
        return Err(anyhow!(
            "transaction metadata txid mismatch: expected {txid}, found {}",
            metadata.txid
        ));
    }
    metadata.status = status;
    write_transaction_metadata(layout, &metadata)?;
    Ok(())
}

pub fn append_transaction_journal_entry(
    layout: &PrefixLayout,
    txid: &str,
    entry: &TransactionJournalEntry,
) -> Result<PathBuf> {
    let path = layout.transaction_journal_path(txid);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    durable::append_line(&path, &serialize_transaction_journal_entry(entry))
        .with_context(|| format!("failed to append transaction journal: {}", path.display()))?;
    Ok(path)
}

pub fn read_transaction_journal_entries(
    layout: &PrefixLayout,
    txid: &str,
) -> Result<Vec<TransactionJournalEntry>> {
    let path = layout.transaction_journal_path(txid);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("failed to read transaction journal: {}", path.display())
            });
        }
    };

    let mut entries = Vec::new();
    for (index, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let entry = serde_json::from_str::<TransactionJournalEntryDocument>(line)
            .map(TransactionJournalEntry::from)
            .with_context(|| {
                format!(
                    "failed parsing transaction journal line {}: {}",
                    index + 1,
                    path.display()
                )
            })?;
        entries.push(entry);
    }

    Ok(entries)
}

pub fn current_unix_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time is before unix epoch")?
        .as_secs())
}

fn serialize_transaction_metadata(metadata: &TransactionMetadata) -> String {
    let document = TransactionMetadataDocument::from(metadata);
    let mut raw = serde_json::to_string_pretty(&document)
        .expect("transaction metadata document should serialize");
    raw.push('\n');
    raw
}

fn serialize_transaction_journal_entry(entry: &TransactionJournalEntry) -> String {
    let mut fields = vec![
        format!("\"seq\":{}", entry.seq),
        format!("\"step\":\"{}\"", escape_json(&entry.step)),
        format!("\"state\":\"{}\"", escape_json(&entry.state)),
    ];
    if let Some(path) = &entry.path {
        fields.push(format!("\"path\":\"{}\"", escape_json(path)));
    }
    format!("{{{}}}", fields.join(","))
}

#[derive(Debug, Deserialize, Serialize)]
struct TransactionJournalEntryDocument {
    seq: u64,
    step: String,
    state: String,
    path: Option<String>,
}

impl From<TransactionJournalEntryDocument> for TransactionJournalEntry {
    fn from(document: TransactionJournalEntryDocument) -> Self {
        Self {
            seq: document.seq,
            step: document.step,
            state: document.state,
            path: document.path,
        }
    }
}

impl From<TransactionJournalEntry> for TransactionJournalEntryDocument {
    fn from(entry: TransactionJournalEntry) -> Self {
        Self {
            seq: entry.seq,
            step: entry.step,
            state: entry.state,
            path: entry.path,
        }
    }
}

fn parse_transaction_metadata(raw: &str) -> Result<TransactionMetadata> {
    match serde_json::from_str::<TransactionMetadataDocument>(raw) {
        Ok(document) => TransactionMetadata::try_from(document),
        Err(serde_error) => {
            if raw.trim_start().starts_with('{') {
                return Err(serde_error).context("invalid transaction metadata JSON");
            }
            match parse_transaction_metadata_legacy(raw) {
                Ok(metadata) => Ok(metadata),
                Err(legacy_error) => Err(legacy_error).context(serde_error),
            }
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct TransactionMetadataDocument {
    version: u32,
    txid: String,
    operation: String,
    status: String,
    started_at_unix: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot_id: Option<String>,
}

impl From<&TransactionMetadata> for TransactionMetadataDocument {
    fn from(metadata: &TransactionMetadata) -> Self {
        Self {
            version: metadata.version,
            txid: metadata.txid.clone(),
            operation: metadata.operation.clone(),
            status: metadata.status.as_str().to_string(),
            started_at_unix: metadata.started_at_unix,
            snapshot_id: metadata.snapshot_id.clone(),
        }
    }
}

impl TryFrom<TransactionMetadataDocument> for TransactionMetadata {
    type Error = anyhow::Error;

    fn try_from(document: TransactionMetadataDocument) -> Result<Self> {
        Ok(Self {
            version: document.version,
            txid: document.txid,
            operation: document.operation,
            status: TransactionStatus::parse(&document.status)?,
            started_at_unix: document.started_at_unix,
            snapshot_id: document.snapshot_id,
        })
    }
}

fn parse_transaction_metadata_legacy(raw: &str) -> Result<TransactionMetadata> {
    let mut string_fields = HashMap::new();
    let mut number_fields = HashMap::new();

    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if line == "{" || line == "}" {
            continue;
        }

        let normalized = line.strip_suffix(',').unwrap_or(line);
        let (raw_key, raw_value) = normalized
            .split_once(':')
            .ok_or_else(|| anyhow!("invalid transaction metadata line: {line}"))?;

        let key = raw_key.trim().trim_matches('"').to_string();
        let value = raw_value.trim();
        if value.starts_with('"') || value.ends_with('"') {
            if !(value.starts_with('"') && value.ends_with('"') && value.len() >= 2) {
                return Err(anyhow!(
                    "invalid quoted transaction metadata value for field: {key}"
                ));
            }

            let inner = &value[1..value.len() - 1];
            string_fields.insert(key, unescape_json(inner)?);
        } else {
            number_fields.insert(key, value.to_string());
        }
    }

    let parse_number = |field: &str| -> Result<u64> {
        number_fields
            .get(field)
            .with_context(|| format!("missing transaction metadata field: {field}"))?
            .parse::<u64>()
            .with_context(|| format!("invalid numeric transaction metadata field: {field}"))
    };

    Ok(TransactionMetadata {
        version: parse_number("version")? as u32,
        txid: string_fields
            .get("txid")
            .with_context(|| "missing transaction metadata field: txid")?
            .clone(),
        operation: string_fields
            .get("operation")
            .with_context(|| "missing transaction metadata field: operation")?
            .clone(),
        status: TransactionStatus::parse(
            string_fields
                .get("status")
                .with_context(|| "missing transaction metadata field: status")?,
        )?,
        started_at_unix: parse_number("started_at_unix")?,
        snapshot_id: string_fields.get("snapshot_id").cloned(),
    })
}

fn escape_json(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn unescape_json(value: &str) -> Result<String> {
    let mut out = String::new();
    let mut chars = value.chars();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }

        let escaped = chars
            .next()
            .ok_or_else(|| anyhow!("unterminated JSON escape sequence"))?;
        match escaped {
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            other => {
                return Err(anyhow!("unsupported JSON escape sequence: \\{other}"));
            }
        }
    }

    Ok(out)
}
