#[derive(Debug)]
enum MetadataBackend {
    Legacy(RegistryIndex),
    Configured(ConfiguredRegistryIndex),
}

impl MetadataBackend {
    fn search_names(&self, query: &str) -> Result<Vec<String>> {
        match self {
            Self::Legacy(index) => index.search_names(query),
            Self::Configured(index) => index.search_names(query),
        }
    }

    fn package_versions(&self, name: &str) -> Result<Vec<PackageManifest>> {
        match self {
            Self::Legacy(index) => index.package_versions(name),
            Self::Configured(index) => index.package_versions(name),
        }
    }

    fn package_versions_with_source(
        &self,
        name: &str,
    ) -> Result<Option<(String, Vec<PackageManifest>)>> {
        match self {
            Self::Legacy(index) => {
                let manifests = index.package_versions(name)?;
                if manifests.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some((index.root().display().to_string(), manifests)))
                }
            }
            Self::Configured(index) => index.package_versions_with_source(name),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SearchMatchKind {
    Exact,
    Prefix,
    Keyword,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchResult {
    name: String,
    description: Option<String>,
    latest_version: String,
    source: String,
    match_kind: SearchMatchKind,
}

fn run_search_command(backend: &MetadataBackend, query: &str) -> Result<Vec<SearchResult>> {
    let query = query.trim();
    let names = backend
        .search_names(query)
        .with_context(|| SEARCH_METADATA_GUIDANCE)?;

    let mut results = Vec::new();
    for name in names {
        let Some(match_kind) = classify_search_match(&name, query) else {
            continue;
        };
        let sourced = backend
            .package_versions_with_source(&name)
            .with_context(|| SEARCH_METADATA_GUIDANCE)?;
        let Some((source, manifests)) = sourced else {
            continue;
        };
        let Some(latest) = manifests.first() else {
            continue;
        };
        results.push(SearchResult {
            name,
            description: best_available_short_description(latest),
            latest_version: latest.version.to_string(),
            source,
            match_kind,
        });
    }

    results.sort_by(|left, right| {
        left.match_kind
            .cmp(&right.match_kind)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.source.cmp(&right.source))
    });
    Ok(results)
}

fn classify_search_match(name: &str, query: &str) -> Option<SearchMatchKind> {
    if name == query {
        return Some(SearchMatchKind::Exact);
    }
    if name.starts_with(query) {
        return Some(SearchMatchKind::Prefix);
    }
    if name.contains(query) {
        return Some(SearchMatchKind::Keyword);
    }
    None
}

fn best_available_short_description(manifest: &PackageManifest) -> Option<String> {
    if !manifest.provides.is_empty() {
        return Some(format!("provides: {}", manifest.provides.join(", ")));
    }
    if let Some(license) = &manifest.license {
        return Some(format!("license: {license}"));
    }
    if let Some(homepage) = &manifest.homepage {
        return Some(format!("homepage: {homepage}"));
    }
    None
}

fn format_search_results(results: &[SearchResult], query: &str) -> Vec<String> {
    if results.is_empty() {
        return vec![format!(
            "No packages found matching '{query}'. Try a broader keyword or run `crosspack update` to refresh local snapshots."
        )];
    }

    let mut lines = Vec::with_capacity(results.len() + 1);
    lines.push("name\tdescription\tlatest\tsource".to_string());
    for result in results {
        lines.push(format!(
            "{}\t{}\t{}\t{}",
            result.name,
            result.description.as_deref().unwrap_or("-"),
            result.latest_version,
            result.source
        ));
    }
    lines
}

fn select_metadata_backend(
    registry_root_override: Option<&Path>,
    layout: &PrefixLayout,
) -> Result<MetadataBackend> {
    if let Some(root) = registry_root_override {
        return Ok(MetadataBackend::Legacy(RegistryIndex::open(root)));
    }

    let source_state_root = registry_state_root(layout);
    let store = RegistrySourceStore::new(&source_state_root);
    let sources = store.list_sources_with_snapshot_state()?;
    let has_ready_snapshot = sources
        .iter()
        .any(|source| matches!(source.snapshot, RegistrySourceSnapshotState::Ready { .. }));
    if sources.is_empty() || !has_ready_snapshot {
        anyhow::bail!(METADATA_CONFIG_GUIDANCE);
    }

    let configured = ConfiguredRegistryIndex::open(source_state_root)
        .with_context(|| "failed loading configured registry snapshots for metadata commands")?;
    Ok(MetadataBackend::Configured(configured))
}

fn resolve_transaction_snapshot_id(layout: &PrefixLayout, operation: &str) -> Result<String> {
    let source_state_root = registry_state_root(layout);
    let store = RegistrySourceStore::new(&source_state_root);
    let sources = store.list_sources_with_snapshot_state()?;

    let mut ready = sources
        .into_iter()
        .filter(|source| source.source.enabled)
        .filter_map(|source| match source.snapshot {
            RegistrySourceSnapshotState::Ready { snapshot_id } => {
                Some((source.source.name, snapshot_id))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    if ready.is_empty() {
        anyhow::bail!(METADATA_CONFIG_GUIDANCE);
    }

    ready.sort_by(|left, right| left.0.cmp(&right.0));
    let snapshot_id = ready[0].1.clone();
    if ready.iter().any(|(_, candidate)| candidate != &snapshot_id) {
        let _ = record_snapshot_id_mismatch(layout, operation, &ready);
        let summary = ready
            .iter()
            .map(|(name, snapshot)| format!("{name}={snapshot}"))
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!("metadata snapshot mismatch across configured sources: {summary}");
    }

    Ok(snapshot_id)
}

fn snapshot_monitor_log_path(layout: &PrefixLayout) -> PathBuf {
    layout.transactions_dir().join("snapshot-monitor.log")
}

fn record_snapshot_id_mismatch(
    layout: &PrefixLayout,
    operation: &str,
    ready: &[(String, String)],
) -> Result<()> {
    fs::create_dir_all(layout.transactions_dir()).with_context(|| {
        format!(
            "failed creating snapshot monitor state dir: {}",
            layout.transactions_dir().display()
        )
    })?;

    let timestamp_unix = current_unix_timestamp()?;
    let source_count = ready.len();
    let unique_snapshot_ids = ready
        .iter()
        .map(|(_, snapshot_id)| snapshot_id.as_str())
        .collect::<HashSet<_>>()
        .len();
    let source_summary = ready
        .iter()
        .map(|(name, snapshot)| format!("{name}={snapshot}"))
        .collect::<Vec<_>>()
        .join(",");
    let monitor_path = snapshot_monitor_log_path(layout);

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&monitor_path)
        .with_context(|| {
            format!(
                "failed opening snapshot monitor log for append: {}",
                monitor_path.display()
            )
        })?;

    writeln!(
        file,
        "timestamp_unix={timestamp_unix} level=error event=snapshot_id_consistency_mismatch error_code={} operation={operation} source_count={source_count} unique_snapshot_ids={unique_snapshot_ids} sources={source_summary}",
        SNAPSHOT_ID_MISMATCH_ERROR_CODE
    )
    .with_context(|| {
        format!(
            "failed writing snapshot monitor log entry: {}",
            monitor_path.display()
        )
    })?;

    Ok(())
}

struct UpdateReport {
    lines: Vec<String>,
    updated: u32,
    up_to_date: u32,
    failed: u32,
}

fn build_update_report(results: &[SourceUpdateResult]) -> UpdateReport {
    let mut updated = 0_u32;
    let mut up_to_date = 0_u32;
    let mut failed = 0_u32;
    let mut lines = Vec::with_capacity(results.len());

    for result in results {
        match result.status {
            SourceUpdateStatus::Updated => {
                updated += 1;
                lines.push(format!("{}: updated", result.name));
            }
            SourceUpdateStatus::UpToDate => {
                up_to_date += 1;
                lines.push(format!("{}: up-to-date", result.name));
            }
            SourceUpdateStatus::Failed => {
                failed += 1;
                let reason = update_failure_reason_code(result.error.as_deref());
                lines.push(format!("{}: failed (reason={reason})", result.name));
            }
        }
    }

    UpdateReport {
        lines,
        updated,
        up_to_date,
        failed,
    }
}

fn ensure_update_succeeded(failed: u32) -> Result<()> {
    if failed > 0 {
        return Err(anyhow!("source update failed"));
    }
    Ok(())
}

fn format_update_summary_line(updated: u32, up_to_date: u32, failed: u32) -> String {
    format!("update summary: updated={updated} up-to-date={up_to_date} failed={failed}")
}

fn update_failure_reason_code(error: Option<&str>) -> String {
    let Some(error) = error else {
        return "unknown".to_string();
    };

    for segment in error.split(':') {
        let candidate = segment.trim();
        if !candidate.is_empty()
            && candidate
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch == '-' || ch.is_ascii_digit())
        {
            return candidate.to_string();
        }
    }

    "unknown".to_string()
}

