use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{PrefixLayout, TransactionJournalEntry, TransactionMetadata};

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

    file.write_all(format!("{txid}\n").as_bytes())
        .with_context(|| {
            format!(
                "failed to write active transaction file: {}",
                path.display()
            )
        })?;
    file.flush().with_context(|| {
        format!(
            "failed to flush active transaction file: {}",
            path.display()
        )
    })?;

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

pub fn clear_active_transaction(layout: &PrefixLayout) -> Result<()> {
    let path = layout.transaction_active_path();
    if path.exists() {
        fs::remove_file(&path).with_context(|| {
            format!(
                "failed to clear active transaction file: {}",
                path.display()
            )
        })?;
    }
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

    fs::write(&path, serialize_transaction_metadata(metadata)).with_context(|| {
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

pub fn update_transaction_status(layout: &PrefixLayout, txid: &str, status: &str) -> Result<()> {
    let mut metadata = read_transaction_metadata(layout, txid)?
        .ok_or_else(|| anyhow!("transaction metadata not found for '{txid}'"))?;
    metadata.status = status.to_string();
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

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open transaction journal: {}", path.display()))?;
    file.write_all(serialize_transaction_journal_entry(entry).as_bytes())
        .with_context(|| format!("failed to append transaction journal: {}", path.display()))?;
    file.write_all(b"\n").with_context(|| {
        format!(
            "failed to append transaction journal newline: {}",
            path.display()
        )
    })?;
    file.flush()
        .with_context(|| format!("failed to flush transaction journal: {}", path.display()))?;
    Ok(path)
}

pub fn current_unix_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time is before unix epoch")?
        .as_secs())
}

fn serialize_transaction_metadata(metadata: &TransactionMetadata) -> String {
    let snapshot_id = metadata
        .snapshot_id
        .as_ref()
        .map(|value| format!("\n  \"snapshot_id\": \"{}\"", escape_json(value)))
        .unwrap_or_default();

    format!(
        "{{\n  \"version\": {},\n  \"txid\": \"{}\",\n  \"operation\": \"{}\",\n  \"status\": \"{}\",\n  \"started_at_unix\": {}{}\n}}\n",
        metadata.version,
        escape_json(&metadata.txid),
        escape_json(&metadata.operation),
        escape_json(&metadata.status),
        metadata.started_at_unix,
        snapshot_id
    )
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

fn parse_transaction_metadata(raw: &str) -> Result<TransactionMetadata> {
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
        status: string_fields
            .get("status")
            .with_context(|| "missing transaction metadata field: status")?
            .clone(),
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
