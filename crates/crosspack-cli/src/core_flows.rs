fn parse_spec(spec: &str) -> Result<(String, VersionReq)> {
    let (name, req) = match spec.split_once('@') {
        Some((name, req)) => (name, req),
        None => (spec, "*"),
    };
    if name.trim().is_empty() {
        return Err(anyhow!("package name must not be empty"));
    }
    let requirement = VersionReq::parse(req)
        .with_context(|| format!("invalid version requirement for '{name}': {req}"))?;
    Ok((name.to_string(), requirement))
}

fn parse_pin_spec(spec: &str) -> Result<(String, VersionReq)> {
    let Some((name, req)) = spec.split_once('@') else {
        return Err(anyhow!(
            "pin requires explicit constraint: use '<name>@<requirement>'"
        ));
    };
    if name.trim().is_empty() {
        return Err(anyhow!("package name must not be empty"));
    }
    if req.trim().is_empty() {
        return Err(anyhow!("pin requirement must not be empty"));
    }

    let requirement = VersionReq::parse(req)
        .with_context(|| format!("invalid pin requirement for '{name}': {req}"))?;
    Ok((name.to_string(), requirement))
}

fn parse_installed_package_selector(
    raw: &str,
    target_flag: Option<String>,
    profile_flag: Option<String>,
    source_flag: Option<String>,
) -> Result<InstalledPackageSelector> {
    let (name_and_target, compact_profile) = match raw.split_once('#') {
        Some((left, right)) if !right.is_empty() => (left, Some(right.to_string())),
        Some(_) => return Err(anyhow!("profile selector must not be empty")),
        None => (raw, None),
    };
    let (package, compact_target) = match name_and_target.split_once('@') {
        Some((left, right)) if !left.is_empty() && !right.is_empty() => {
            (left.to_string(), Some(right.to_string()))
        }
        Some(_) => return Err(anyhow!("target selector must include package and target")),
        None => (name_and_target.to_string(), None),
    };
    if package.trim().is_empty() {
        return Err(anyhow!("package selector must not be empty"));
    }
    if compact_target.is_some() && target_flag.is_some() {
        return Err(anyhow!("target specified twice"));
    }
    if compact_profile.is_some() && profile_flag.is_some() {
        return Err(anyhow!("profile specified twice"));
    }

    Ok(InstalledPackageSelector {
        package,
        target: target_flag.or(compact_target),
        profile: profile_flag.or(compact_profile),
        source_namespace: source_flag,
    })
}

fn parse_provider_overrides(values: &[String]) -> Result<BTreeMap<String, String>> {
    let mut overrides = BTreeMap::new();
    for value in values {
        let (capability, package) = value.split_once('=').ok_or_else(|| {
            anyhow!(
                "invalid provider override '{}': expected capability=package",
                value
            )
        })?;

        if !is_policy_token(capability) {
            return Err(anyhow!(
                "invalid provider override '{}': capability '{}' must use package-name grammar",
                value,
                capability
            ));
        }
        if !is_policy_token(package) {
            return Err(anyhow!(
                "invalid provider override '{}': package '{}' must use package-name grammar",
                value,
                package
            ));
        }

        if overrides
            .insert(capability.to_string(), package.to_string())
            .is_some()
        {
            return Err(anyhow!(
                "invalid provider override '{}': duplicate override for capability '{}': use one binding per capability",
                value,
                capability
            ));
        }
    }

    Ok(overrides)
}

fn is_policy_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }

    let starts_valid = bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit();
    starts_valid
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._+-".contains(b))
}

fn format_info_lines(name: &str, versions: &[PackageManifest]) -> Vec<String> {
    let mut manifests = versions.iter().collect::<Vec<_>>();
    manifests.sort_by(|left, right| right.version.cmp(&left.version));

    let mut lines = vec![format!("Package: {name}")];
    for manifest in manifests {
        lines.push(format!("- {}", manifest.version));

        if let Some(description) = manifest.description.as_deref() {
            let trimmed = description.trim();
            if !trimmed.is_empty() {
                lines.push(format!(
                    "  Description: {}",
                    sanitize_metadata_cell(trimmed)
                ));
            }
        }

        if !manifest.provides.is_empty() {
            lines.push(format!("  Provides: {}", manifest.provides.join(", ")));
        }

        if !manifest.conflicts.is_empty() {
            let conflicts = manifest
                .conflicts
                .iter()
                .map(|(name, req)| format!("{}({})", name, req))
                .collect::<Vec<_>>();
            lines.push(format!("  Conflicts: {}", conflicts.join(", ")));
        }

        if !manifest.replaces.is_empty() {
            let replaces = manifest
                .replaces
                .iter()
                .map(|(name, req)| format!("{}({})", name, req))
                .collect::<Vec<_>>();
            lines.push(format!("  Replaces: {}", replaces.join(", ")));
        }

        if !manifest.provides.is_empty()
            || !manifest.conflicts.is_empty()
            || !manifest.replaces.is_empty()
        {
            lines.push(format!(
                "  Policy: provides={} conflicts={} replaces={}",
                manifest.provides.len(),
                manifest.conflicts.len(),
                manifest.replaces.len()
            ));
        }
    }

    lines
}

fn format_info_lines_for_style(
    style: OutputStyle,
    name: &str,
    versions: &[PackageManifest],
) -> Vec<String> {
    if style == OutputStyle::Plain {
        return format_info_lines(name, versions);
    }

    let mut manifests = versions.iter().collect::<Vec<_>>();
    manifests.sort_by(|left, right| right.version.cmp(&left.version));

    if manifests.is_empty() {
        return render_empty_state(style, &format!("No package found: {name}"), None);
    }

    let mut lines = vec![render_status_line(style, "ok", name)];
    for manifest in manifests {
        lines.push(render_key_value_detail(
            style,
            "version",
            &manifest.version.to_string(),
        ));
        if let Some(description) = best_available_short_description(manifest) {
            lines.push(render_key_value_detail(style, "summary", &description));
        }
        if let Some(homepage) = &manifest.homepage {
            lines.push(render_key_value_detail(style, "homepage", homepage));
        }
        if let Some(license) = &manifest.license {
            lines.push(render_key_value_detail(style, "license", license));
        }
        if !manifest.provides.is_empty() {
            lines.push(render_key_value_detail(
                style,
                "provides",
                &manifest.provides.join(", "),
            ));
        }
        if !manifest.conflicts.is_empty() {
            let conflicts = manifest
                .conflicts
                .iter()
                .map(|(name, req)| format!("{}({})", name, req))
                .collect::<Vec<_>>();
            lines.push(render_key_value_detail(
                style,
                "conflicts",
                &conflicts.join(", "),
            ));
        }
        if !manifest.replaces.is_empty() {
            let replaces = manifest
                .replaces
                .iter()
                .map(|(name, req)| format!("{}({})", name, req))
                .collect::<Vec<_>>();
            lines.push(render_key_value_detail(
                style,
                "replaces",
                &replaces.join(", "),
            ));
        }
        if !manifest.provides.is_empty()
            || !manifest.conflicts.is_empty()
            || !manifest.replaces.is_empty()
        {
            lines.push(render_key_value_detail(
                style,
                "policy",
                &format!(
                    "provides={} conflicts={} replaces={}",
                    manifest.provides.len(),
                    manifest.conflicts.len(),
                    manifest.replaces.len()
                ),
            ));
        }
    }
    lines
}

fn apply_provider_override(
    requested_name: &str,
    candidates: Vec<PackageManifest>,
    provider_overrides: &BTreeMap<String, String>,
) -> Result<Vec<PackageManifest>> {
    let Some(provider_name) = provider_overrides.get(requested_name) else {
        return Ok(candidates);
    };

    let has_direct_package_candidates = candidates
        .iter()
        .any(|manifest| manifest.name == requested_name);
    if has_direct_package_candidates && provider_name != requested_name {
        return Err(anyhow!(
            "provider override '{}={}' is invalid: '{}' resolves directly to package manifests; direct package names cannot be overridden",
            requested_name,
            provider_name,
            requested_name
        ));
    }

    let mut provider_name_seen = false;
    let filtered = candidates
        .into_iter()
        .filter(|manifest| {
            if manifest.name == *provider_name {
                provider_name_seen = true;
            }
            manifest.name == *provider_name
                && (manifest.name == requested_name
                    || manifest
                        .provides
                        .iter()
                        .any(|provided| provided == requested_name))
        })
        .collect::<Vec<_>>();

    if filtered.is_empty() {
        if provider_name_seen {
            return Err(anyhow!(
                "provider override '{}={}' is invalid: package '{}' does not provide capability '{}'",
                requested_name,
                provider_name,
                provider_name,
                requested_name
            ));
        }
        return Err(anyhow!(
            "provider override '{}={}' references unknown provider package '{}'",
            requested_name,
            provider_name,
            provider_name
        ));
    }

    Ok(filtered)
}

fn validate_provider_overrides_used(
    provider_overrides: &BTreeMap<String, String>,
    resolved_dependency_tokens: &HashSet<String>,
) -> Result<()> {
    let unused = provider_overrides
        .iter()
        .filter(|(capability, _)| !resolved_dependency_tokens.contains(*capability))
        .map(|(capability, provider)| format!("{capability}={provider}"))
        .collect::<Vec<_>>();

    if unused.is_empty() {
        return Ok(());
    }

    Err(anyhow!(
        "unused provider override(s): {}",
        unused.join(", ")
    ))
}

#[cfg(test)]
fn select_manifest_with_pin<'a>(
    versions: &'a [PackageManifest],
    request_requirement: &VersionReq,
    pin_requirement: Option<&VersionReq>,
) -> Option<&'a PackageManifest> {
    versions
        .iter()
        .filter(|manifest| request_requirement.matches(&manifest.version))
        .filter(|manifest| {
            pin_requirement
                .map(|pin| pin.matches(&manifest.version))
                .unwrap_or(true)
        })
        .max_by(|a, b| a.version.cmp(&b.version))
}

#[derive(Debug, Clone)]
struct ResolvedInstall {
    manifest: PackageManifest,
    artifact: Artifact,
    resolved_target: String,
    archive_type: ArchiveType,
    source_build: Option<SourceBuildPlan>,
}

#[derive(Debug, Clone)]
struct SourceBuildPlan {
    url: String,
    archive_sha256: String,
    build_system: String,
    build_commands: Vec<String>,
    install_commands: Vec<String>,
    archive_type: ArchiveType,
}

#[derive(Debug, Clone)]
struct InstallOutcome {
    name: String,
    version: String,
    resolved_target: String,
    archive_type: ArchiveType,
    artifact_url: String,
    cache_path: PathBuf,
    download_status: &'static str,
    install_root: PathBuf,
    receipt_path: PathBuf,
    exposed_bins: Vec<String>,
    exposed_completions: Vec<String>,
    exposed_gui_assets: Vec<String>,
    exposed_integrations: Vec<String>,
    native_gui_records: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct PlannedRemoval {
    name: String,
    version: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct PlannedReplacement {
    from_name: String,
    from_version: String,
    to_name: String,
    to_version: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct PlannedTransition {
    name: String,
    from_version: String,
    to_version: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct PlannedAdd {
    name: String,
    version: String,
    target: String,
}

#[derive(Debug, Clone)]
struct PlannedPackageChange {
    name: String,
    target: String,
    new_version: String,
    old_version: Option<String>,
    replacement_removals: Vec<PlannedRemoval>,
}

#[derive(Debug, Clone)]
struct TransactionPreview {
    operation: String,
    adds: Vec<PlannedAdd>,
    removals: Vec<PlannedRemoval>,
    replacements: Vec<PlannedReplacement>,
    transitions: Vec<PlannedTransition>,
    risk_flags: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct DependencyPolicyExplainability {
    provider_substitutions: Vec<PolicyProviderSubstitution>,
    replacement_removals: Vec<PolicyReplacementRemoval>,
    conflict_constraints: Vec<PolicyConflictConstraint>,
}

#[derive(Debug, Clone)]
struct PolicyProviderSubstitution {
    capability: String,
    selected_package: String,
    selected_version: String,
}

#[derive(Debug, Clone)]
struct PolicyReplacementRemoval {
    selected_package: String,
    selected_version: String,
    removed_package: String,
    removed_version: String,
    replacement_requirement: String,
}

#[derive(Debug, Clone)]
struct PolicyConflictConstraint {
    selected_package: String,
    selected_version: String,
    conflict_package: String,
    conflict_requirement: String,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum TransactionPreviewMode {
    DryRun,
}

impl TransactionPreviewMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::DryRun => "dry-run",
        }
    }
}

#[derive(Debug, Clone)]
struct RootInstallRequest {
    name: String,
    requirement: VersionReq,
}

#[derive(Debug, Clone)]
struct UpgradePlan {
    target: Option<String>,
    roots: Vec<RootInstallRequest>,
    root_names: Vec<String>,
}

#[derive(Debug, Clone)]
struct TransactionJournalRecord {
    seq: u64,
    step: String,
    state: String,
    path: Option<String>,
}

#[derive(Debug, Clone)]
struct PackageSnapshotManifest {
    package_exists: bool,
    receipt_exists: bool,
    bins: Vec<String>,
    completions: Vec<String>,
    gui_assets: Vec<GuiExposureAsset>,
    integrations: Vec<IntegrationProjection>,
    shell_init: Vec<ShellInitProjection>,
    native_sidecar_exists: bool,
    declared_services_sidecar_exists: bool,
}

fn begin_transaction(
    layout: &PrefixLayout,
    operation: &str,
    snapshot_id: Option<&str>,
    started_at_unix: u64,
) -> Result<TransactionMetadata> {
    Ok(TransactionCoordinator::new(layout)
        .begin(operation, snapshot_id, started_at_unix)?
        .metadata)
}

fn set_transaction_status(layout: &PrefixLayout, txid: &str, status: TransactionStatus) -> Result<()> {
    let coordinator = TransactionCoordinator::new(layout);
    match status {
        TransactionStatus::Planning => update_transaction_status(layout, txid, status),
        TransactionStatus::Applying => coordinator.mark_applying(txid),
        TransactionStatus::Completed => update_transaction_status(layout, txid, status),
        TransactionStatus::Committed => coordinator.mark_committed(txid),
        TransactionStatus::RollingBack => coordinator.mark_rolling_back(txid),
        TransactionStatus::RolledBack => coordinator.mark_rolled_back(txid),
        TransactionStatus::Failed => coordinator.mark_failed(txid),
    }
}

fn resolve_unambiguous_installed_package(
    layout: &PrefixLayout,
    package_name: &str,
) -> Result<Option<InstalledPackageState>> {
    resolve_installed_selector_for_cli(
        layout,
        &InstalledPackageSelector {
            package: package_name.to_string(),
            target: None,
            profile: None,
            source_namespace: None,
        },
    )
}

fn resolve_installed_selector_for_cli(
    layout: &PrefixLayout,
    selector: &InstalledPackageSelector,
) -> Result<Option<InstalledPackageState>> {
    match resolve_installed_package_selector(layout, selector)? {
        Ok(state) => Ok(state),
        Err(ambiguity) => {
            let choices = ambiguity
                .matches
                .iter()
                .map(|state| format!("  {}", state.identity.selector_display()))
                .collect::<Vec<_>>()
                .join("\n");
            Err(anyhow!(
                "installed package name '{}' is ambiguous; specify one of:\n{}",
                ambiguity.selector.package,
                choices
            ))
        }
    }
}

fn execute_with_transaction<F>(
    layout: &PrefixLayout,
    operation: &str,
    snapshot_id: Option<&str>,
    run: F,
) -> Result<()>
where
    F: FnOnce(&TransactionMetadata) -> Result<()>,
{
    let started_at_unix = current_unix_timestamp()?;
    let tx = begin_transaction(layout, operation, snapshot_id, started_at_unix)?;

    let run_result = (|| -> Result<()> {
        set_transaction_status(layout, &tx.txid, TransactionStatus::Applying)?;
        run(&tx)?;
        set_transaction_status(layout, &tx.txid, TransactionStatus::Committed)?;
        TransactionCoordinator::new(layout).clear_active()?;
        Ok(())
    })();

    match run_result {
        Ok(()) => Ok(()),
        Err(err) => {
            let current_status = read_transaction_metadata(layout, &tx.txid)
                .ok()
                .flatten()
                .and_then(|metadata| (metadata.txid == tx.txid).then_some(metadata.status));
            let preserve_recovery_state = current_status
                .as_ref()
                .map(|status| {
                    matches!(
                        status,
                        TransactionStatus::RollingBack
                            | TransactionStatus::RolledBack
                            | TransactionStatus::Completed
                            | TransactionStatus::Committed
                            | TransactionStatus::Failed
                    )
                })
                .unwrap_or(false);
            if matches!(
                current_status,
                Some(
                    TransactionStatus::RolledBack
                        | TransactionStatus::Completed
                        | TransactionStatus::Committed
                )
            ) {
                let _ = TransactionCoordinator::new(layout).clear_active();
            }
            if !preserve_recovery_state {
                let _ = set_transaction_status(layout, &tx.txid, TransactionStatus::Failed);
            }
            Err(err)
        }
    }
}

fn normalize_command_token(command: &str) -> String {
    let command = command.trim().to_ascii_lowercase();
    if command.is_empty() {
        "unknown".to_string()
    } else {
        command
    }
}

fn ensure_no_active_transaction_for(layout: &PrefixLayout, command: &str) -> Result<()> {
    let command = normalize_command_token(command);
    ensure_no_active_transaction(layout).map_err(|err| {
        anyhow!("cannot {command} (reason=active_transaction command={command}): {err}")
    })
}

fn ensure_no_active_transaction(layout: &PrefixLayout) -> Result<()> {
    let action = TransactionCoordinator::new(layout).classify_recovery()?;
    match action {
        TransactionRecoveryAction::Clean
        | TransactionRecoveryAction::FinalizeCommitted { .. }
        | TransactionRecoveryAction::ClearRolledBack { .. } => {
            TransactionCoordinator::new(layout).clear_active()?;
            Ok(())
        }
        TransactionRecoveryAction::CleanupPlanning { txid } => Err(anyhow!(
            "transaction {txid} is active (reason=active_status status=planning)"
        )),
        TransactionRecoveryAction::Rollback { txid } => Err(anyhow!(
            "transaction {txid} is active (reason=active_status status=applying)"
        )),
        TransactionRecoveryAction::ResumeRollback { txid } => Err(anyhow!(
            "transaction {txid} requires repair (reason=rolling_back)"
        )),
        TransactionRecoveryAction::BlockedFailed { txid } => Err(anyhow!(
            "transaction {txid} requires repair (reason=failed)"
        )),
        TransactionRecoveryAction::RepairRequired(reason) => {
            Err(anyhow!(format_transaction_preflight_required(layout, &reason)))
        }
    }
}

fn doctor_transaction_health_line(layout: &PrefixLayout) -> Result<String> {
    let action = TransactionCoordinator::new(layout).classify_recovery()?;
    Ok(match action {
        TransactionRecoveryAction::Clean => "transaction: clean".to_string(),
        TransactionRecoveryAction::FinalizeCommitted { .. }
        | TransactionRecoveryAction::ClearRolledBack { .. } => {
            TransactionCoordinator::new(layout).clear_active()?;
            "transaction: clean".to_string()
        }
        TransactionRecoveryAction::CleanupPlanning { txid } => format!("transaction: active {txid}"),
        TransactionRecoveryAction::Rollback { txid } => format!("transaction: active {txid}"),
        TransactionRecoveryAction::ResumeRollback { txid } => {
            format!("transaction: failed {txid} (reason=rolling_back)")
        }
        TransactionRecoveryAction::BlockedFailed { txid } => {
            format!("transaction: failed {txid} (reason=failed)")
        }
        TransactionRecoveryAction::RepairRequired(reason) => {
            format_transaction_repair_failed_line(layout, &reason)
        }
    })
}

fn doctor_transaction_detail_line(layout: &PrefixLayout) -> Result<Option<String>> {
    let Ok(ActiveTransactionMarker::Present(txid)) = read_active_transaction_marker(layout) else {
        return Ok(None);
    };
    let Ok(Some(metadata)) = read_transaction_metadata(layout, &txid) else {
        return Ok(None);
    };
    if metadata.txid != txid {
        return Ok(None);
    }
    match format_transaction_detail_line(layout, &metadata) {
        Ok(line) => Ok(Some(line)),
        Err(_) => Ok(None),
    }
}

fn format_transaction_detail_line(
    layout: &PrefixLayout,
    metadata: &TransactionMetadata,
) -> Result<String> {
    let step = latest_transaction_journal_step(layout, &metadata.txid)?.unwrap_or_else(|| "none".to_string());
    Ok(format!(
        "transaction_detail txid={} status={} operation={} step={}",
        metadata.txid, metadata.status, metadata.operation, step
    ))
}

fn latest_transaction_journal_step(layout: &PrefixLayout, txid: &str) -> Result<Option<String>> {
    let entries = match read_transaction_journal_records(layout, txid) {
        Ok(entries) => entries,
        Err(err) => {
            if layout.transaction_journal_path(txid).exists() {
                return Err(err);
            }
            return Ok(None);
        }
    };
    Ok(entries.into_iter().max_by_key(|entry| entry.seq).map(|entry| entry.step))
}

fn format_transaction_repair_failed_line(
    layout: &PrefixLayout,
    reason: &TransactionRepairReason,
) -> String {
    match reason {
        TransactionRepairReason::ActiveMarkerUnreadable => format!(
            "transaction: failed (reason=active_marker_unreadable path={})",
            layout.transaction_active_path().display()
        ),
        TransactionRepairReason::ActiveMarkerInvalid { path } => {
            format!("transaction: failed (reason=active_marker_invalid path={path})")
        }
        TransactionRepairReason::ActiveMarkerWithoutMetadata { txid } => format!(
            "transaction: failed {txid} (reason=metadata_missing path={})",
            layout.transaction_metadata_path(txid).display()
        ),
        TransactionRepairReason::MetadataUnreadable { txid } => format!(
            "transaction: failed {txid} (reason=metadata_unreadable path={})",
            layout.transaction_metadata_path(txid).display()
        ),
        TransactionRepairReason::MetadataTxidMismatch { expected, actual } => format!(
            "transaction: failed {expected} (reason=metadata_txid_mismatch expected={expected} actual={actual})"
        ),
        TransactionRepairReason::JournalUnreadable { txid } => format!(
            "transaction: failed {txid} (reason=journal_unreadable path={})",
            layout.transaction_journal_path(txid).display()
        ),
        TransactionRepairReason::ApplyingWithoutActiveMarker { txid } => {
            format!("transaction: failed {txid} (reason=applying_without_active_marker)")
        }
        TransactionRepairReason::RollbackEvidenceMissing { txid } => {
            format!("transaction: failed {txid} (reason=rollback_evidence_missing)")
        }
    }
}

fn format_transaction_preflight_required(
    layout: &PrefixLayout,
    reason: &TransactionRepairReason,
) -> String {
    match reason {
        TransactionRepairReason::ActiveMarkerUnreadable => format!(
            "transaction state requires repair (reason=active_marker_unreadable path={})",
            layout.transaction_active_path().display()
        ),
        TransactionRepairReason::ActiveMarkerInvalid { path } => {
            format!("transaction state requires repair (reason=active_marker_invalid path={path})")
        }
        TransactionRepairReason::ActiveMarkerWithoutMetadata { txid } => format!(
            "transaction {txid} requires repair (reason=metadata_missing path={})",
            layout.transaction_metadata_path(txid).display()
        ),
        TransactionRepairReason::MetadataUnreadable { txid } => format!(
            "transaction {txid} requires repair (reason=metadata_unreadable path={})",
            layout.transaction_metadata_path(txid).display()
        ),
        TransactionRepairReason::MetadataTxidMismatch { expected, actual } => format!(
            "transaction state requires repair {expected} (reason=metadata_txid_mismatch expected={expected} actual={actual})"
        ),
        TransactionRepairReason::JournalUnreadable { txid } => format!(
            "transaction {txid} requires repair (reason=journal_unreadable path={})",
            layout.transaction_journal_path(txid).display()
        ),
        TransactionRepairReason::ApplyingWithoutActiveMarker { txid } => {
            format!("transaction {txid} requires repair (reason=applying_without_active_marker)")
        }
        TransactionRepairReason::RollbackEvidenceMissing { txid } => {
            format!("transaction {txid} requires repair (reason=rollback_evidence_missing)")
        }
    }
}

fn doctor_installed_state_line(layout: &PrefixLayout) -> String {
    match read_all_installed_package_states(layout) {
        Ok(_) => "installed_state: clean".to_string(),
        Err(err) if err.to_string().contains("duplicate installed identity") => {
            "installed_state: error duplicate-installed-identity".to_string()
        }
        Err(err) => format!("installed_state: error read-failed detail={err}"),
    }
}

fn resolve_install_graph(
    layout: &PrefixLayout,
    index: &MetadataBackend,
    roots: &[RootInstallRequest],
    requested_target: Option<&str>,
    provider_overrides: &BTreeMap<String, String>,
    build_from_source: bool,
) -> Result<Vec<ResolvedInstall>> {
    let (resolved, _) = resolve_install_graph_with_tokens(
        layout,
        index,
        roots,
        requested_target,
        provider_overrides,
        true,
        build_from_source,
    )?;
    Ok(resolved)
}

fn resolve_install_graph_with_tokens(
    layout: &PrefixLayout,
    index: &MetadataBackend,
    roots: &[RootInstallRequest],
    requested_target: Option<&str>,
    provider_overrides: &BTreeMap<String, String>,
    validate_overrides: bool,
    build_from_source: bool,
) -> Result<(Vec<ResolvedInstall>, HashSet<String>)> {
    let mut pins = BTreeMap::new();
    for (name, raw_req) in read_all_pins(layout)? {
        let parsed = VersionReq::parse(&raw_req)
            .with_context(|| format!("invalid pin requirement for '{name}' in state: {raw_req}"))?;
        pins.insert(name, parsed);
    }

    let root_reqs: Vec<RootRequirement> = roots
        .iter()
        .map(|root| RootRequirement {
            name: root.name.clone(),
            requirement: root.requirement.clone(),
        })
        .collect();

    let installed = installed_manifests_for_receipts(index, &read_install_receipts(layout)?)?;
    let graph = resolve_dependency_graph_with_installed_manifests(&root_reqs, &pins, &installed, |package_name| {
        let versions = index.dependency_versions(package_name)?;
        apply_provider_override(package_name, versions, provider_overrides)
    })?;

    let resolved_dependency_tokens = graph.manifests.keys().cloned().collect::<HashSet<_>>();
    if validate_overrides {
        validate_provider_overrides_used(provider_overrides, &resolved_dependency_tokens)?;
    }

    let resolved_target = requested_target
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| host_target_triple().to_string());

    let resolved = graph
        .install_order
        .iter()
        .map(|name| {
            let manifest = graph
                .manifests
                .get(name)
                .ok_or_else(|| anyhow!("resolver selected package missing from graph: {name}"))?
                .clone();

            let (artifact, source_build) =
                select_install_plan_for_target(&manifest, &resolved_target, build_from_source)?;
            let archive_type = source_build
                .as_ref()
                .map(|plan| plan.archive_type)
                .unwrap_or(artifact.archive_type()?);

            Ok(ResolvedInstall {
                manifest,
                artifact,
                resolved_target: resolved_target.clone(),
                archive_type,
                source_build,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok((resolved, resolved_dependency_tokens))
}

fn installed_manifests_for_receipts(
    index: &MetadataBackend,
    receipts: &[InstallReceipt],
) -> Result<Vec<PackageManifest>> {
    let mut installed = Vec::new();
    for receipt in receipts {
        if let Some(manifest) = index
            .package_versions(&receipt.name)?
            .into_iter()
            .find(|manifest| manifest.version.to_string() == receipt.version)
        {
            installed.push(manifest);
        }
    }
    Ok(installed)
}

fn ensure_explain_requires_dry_run(operation: &str, dry_run: bool, explain: bool) -> Result<()> {
    if explain && !dry_run {
        return Err(anyhow!("--explain requires --dry-run for '{}'", operation));
    }
    Ok(())
}

#[cfg(test)]
fn select_artifact_for_target(
    manifest: &PackageManifest,
    resolved_target: &str,
    build_from_source: bool,
) -> Result<Artifact> {
    let (artifact, _) =
        select_install_plan_for_target(manifest, resolved_target, build_from_source)?;
    Ok(artifact)
}

fn select_install_plan_for_target(
    manifest: &PackageManifest,
    resolved_target: &str,
    build_from_source: bool,
) -> Result<(Artifact, Option<SourceBuildPlan>)> {
    if build_from_source {
        let source = manifest.source_build.as_ref().ok_or_else(|| {
            anyhow!(
                "source build requested for {} {} on target {} but manifest has no source_build metadata",
                manifest.name,
                manifest.version,
                resolved_target
            )
        })?;
        let artifact = select_source_build_artifact_template(manifest, resolved_target)?;
        let plan = validate_source_build_plan(manifest, resolved_target, source)?;
        return Ok((artifact, Some(plan)));
    }

    if let Some(artifact) = manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.target == resolved_target)
    {
        return Ok((artifact.clone(), None));
    }

    if manifest.source_build.is_some() {
        return Err(anyhow!(
            "source build required for {} {} on target {}: no binary artifact published; rerun with --build-from-source",
            manifest.name,
            manifest.version,
            resolved_target
        ));
    }

    Err(anyhow!(
        "no artifact available for target {} in {} {}",
        resolved_target,
        manifest.name,
        manifest.version
    ))
}

fn select_source_build_artifact_template(
    manifest: &PackageManifest,
    resolved_target: &str,
) -> Result<Artifact> {
    if let Some(artifact) = manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.target == resolved_target)
    {
        return Ok(artifact.clone());
    }

    if manifest.artifacts.len() == 1 {
        return Ok(manifest.artifacts[0].clone());
    }

    Err(anyhow!(
        "source build for {} {} on target {} requires deterministic artifact metadata (expected exactly one artifact template when target-specific artifact is absent)",
        manifest.name,
        manifest.version,
        resolved_target
    ))
}

fn validate_source_build_plan(
    manifest: &PackageManifest,
    resolved_target: &str,
    source: &crosspack_core::SourceBuildMetadata,
) -> Result<SourceBuildPlan> {
    let url = source.url.trim();
    if url.is_empty() {
        return Err(anyhow!(
            "invalid source_build metadata for {} {} on target {}: url must not be empty",
            manifest.name,
            manifest.version,
            resolved_target
        ));
    }
    let build_system = source.build_system.trim();
    let archive_sha256 = source.archive_sha256.trim();
    if archive_sha256.is_empty() {
        return Err(anyhow!(
            "invalid source_build metadata for {} {} on target {}: archive_sha256 must not be empty",
            manifest.name,
            manifest.version,
            resolved_target
        ));
    }
    if !is_valid_sha256_hex(archive_sha256) {
        return Err(anyhow!(
            "invalid source_build metadata for {} {} on target {}: archive_sha256 must be a 64-character hexadecimal SHA-256 digest",
            manifest.name,
            manifest.version,
            resolved_target
        ));
    }
    if build_system.is_empty() {
        return Err(anyhow!(
            "invalid source_build metadata for {} {} on target {}: build_system must not be empty",
            manifest.name,
            manifest.version,
            resolved_target
        ));
    }
    if source.build_commands.is_empty() {
        return Err(anyhow!(
            "invalid source_build metadata for {} {} on target {}: build_commands must not be empty",
            manifest.name,
            manifest.version,
            resolved_target
        ));
    }
    if source.install_commands.is_empty() {
        return Err(anyhow!(
            "invalid source_build metadata for {} {} on target {}: install_commands must not be empty",
            manifest.name,
            manifest.version,
            resolved_target
        ));
    }
    if source
        .build_commands
        .iter()
        .chain(source.install_commands.iter())
        .any(|token| token.trim().is_empty())
    {
        return Err(anyhow!(
            "invalid source_build metadata for {} {} on target {}: command tokens must not be empty",
            manifest.name,
            manifest.version,
            resolved_target
        ));
    }

    let archive_type = ArchiveType::infer_from_url(url).ok_or_else(|| {
        anyhow!(
            "invalid source_build metadata for {} {} on target {}: url '{}' must include a supported archive extension",
            manifest.name,
            manifest.version,
            resolved_target,
            url
        )
    })?;
    if !matches!(
        archive_type,
        ArchiveType::Zip | ArchiveType::TarGz | ArchiveType::TarZst
    ) {
        return Err(anyhow!(
            "invalid source_build metadata for {} {} on target {}: archive type '{}' is not supported for source builds",
            manifest.name,
            manifest.version,
            resolved_target,
            archive_type.as_str()
        ));
    }

    Ok(SourceBuildPlan {
        url: url.to_string(),
        archive_sha256: archive_sha256.to_string(),
        build_system: build_system.to_string(),
        build_commands: source.build_commands.clone(),
        install_commands: source.install_commands.clone(),
        archive_type,
    })
}

fn build_planned_package_changes(
    resolved: &[ResolvedInstall],
    receipts: &[InstallReceipt],
) -> Result<Vec<PlannedPackageChange>> {
    let mut planned = Vec::with_capacity(resolved.len());
    for package in resolved {
        let replacement_receipts = collect_replacement_receipts(&package.manifest, receipts)?;
        let replacement_removals = replacement_receipts
            .into_iter()
            .map(|receipt| PlannedRemoval {
                name: receipt.name,
                version: receipt.version,
            })
            .collect::<Vec<_>>();
        let old_version = receipts
            .iter()
            .find(|receipt| receipt.name == package.manifest.name)
            .map(|receipt| receipt.version.clone());
        planned.push(PlannedPackageChange {
            name: package.manifest.name.clone(),
            target: package.resolved_target.clone(),
            new_version: package.manifest.version.to_string(),
            old_version,
            replacement_removals,
        });
    }

    planned.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(planned)
}

fn build_transaction_preview(
    operation: &str,
    planned: &[PlannedPackageChange],
) -> TransactionPreview {
    let mut adds = Vec::new();
    let mut removals = BTreeSet::new();
    let mut replacements = BTreeSet::new();
    let mut transitions = Vec::new();

    for package in planned {
        if package.old_version.is_none() {
            adds.push(PlannedAdd {
                name: package.name.clone(),
                version: package.new_version.clone(),
                target: package.target.clone(),
            });
        }

        if let Some(old_version) = package.old_version.as_ref() {
            if old_version != &package.new_version {
                transitions.push(PlannedTransition {
                    name: package.name.clone(),
                    from_version: old_version.clone(),
                    to_version: package.new_version.clone(),
                });
            }
        }

        for removal in &package.replacement_removals {
            removals.insert(removal.clone());
            replacements.insert(PlannedReplacement {
                from_name: removal.name.clone(),
                from_version: removal.version.clone(),
                to_name: package.name.clone(),
                to_version: package.new_version.clone(),
            });
        }
    }

    adds.sort();
    transitions.sort();
    let removals = removals.into_iter().collect::<Vec<_>>();
    let replacements = replacements.into_iter().collect::<Vec<_>>();

    let mut risk_flags = BTreeSet::new();
    if !adds.is_empty() {
        risk_flags.insert("adds".to_string());
    }
    if !removals.is_empty() {
        risk_flags.insert("removals".to_string());
    }
    if !replacements.is_empty() {
        risk_flags.insert("replacements".to_string());
    }
    if !transitions.is_empty() {
        risk_flags.insert("version-transitions".to_string());
    }
    let mut mutating_packages = BTreeSet::new();
    for package in planned {
        let has_add = package.old_version.is_none();
        let has_transition = package
            .old_version
            .as_ref()
            .is_some_and(|old| old != &package.new_version);
        let has_replacement = !package.replacement_removals.is_empty();
        if has_add || has_transition || has_replacement {
            mutating_packages.insert(package.name.clone());
        }
    }
    if mutating_packages.len() > 1 {
        risk_flags.insert("multi-package-transaction".to_string());
    }
    if risk_flags.is_empty() {
        risk_flags.insert("none".to_string());
    }

    TransactionPreview {
        operation: operation.to_string(),
        adds,
        removals,
        replacements,
        transitions,
        risk_flags: risk_flags.into_iter().collect(),
    }
}

fn build_install_plan_from_resolved(
    operation: PlanOperation,
    target: Option<String>,
    resolved: &[ResolvedInstall],
    receipts: &[InstallReceipt],
    roots: &[RootInstallRequest],
) -> InstallPlan {
    let manifests = resolved
        .iter()
        .map(|package| (package.manifest.name.clone(), package.manifest.clone()))
        .collect::<BTreeMap<_, _>>();
    let install_order = resolved
        .iter()
        .map(|package| package.manifest.name.clone())
        .collect::<Vec<_>>();
    let graph = ResolvedGraph {
        manifests,
        install_order,
    };
    let installed = receipts
        .iter()
        .map(|receipt| InstalledPackageSummary {
            name: receipt.name.clone(),
            version: receipt.version.clone(),
            dependencies: receipt.dependencies.clone(),
            install_reason: if receipt.install_reason == InstallReason::Root {
                "root".to_string()
            } else {
                "dependency".to_string()
            },
        })
        .collect::<Vec<_>>();
    let root_names = roots.iter().map(|root| root.name.clone()).collect::<Vec<_>>();

    let mut plan = plan_from_resolved_graph_with_installed(operation, target, &graph, &installed, &root_names);
    let resolved_targets = resolved
        .iter()
        .map(|package| {
            (
                package.manifest.name.as_str(),
                package.resolved_target.as_str(),
            )
        })
        .collect::<HashMap<_, _>>();
    for package in &mut plan.packages {
        if let Some(target) = resolved_targets.get(package.name.as_str()) {
            package.target = (*target).to_string();
        }
    }
    plan
}

fn merge_install_plans(
    operation: PlanOperation,
    target: Option<String>,
    plans: &[InstallPlan],
) -> InstallPlan {
    let mut packages = BTreeMap::new();
    let mut removals = BTreeMap::new();
    let mut replacements = BTreeMap::new();
    let mut transitions = BTreeMap::new();
    let mut provider_substitutions = BTreeMap::new();
    let mut conflicts = BTreeMap::new();

    for plan in plans {
        for package in &plan.packages {
            packages.insert(package.name.clone(), package.clone());
        }
        for removal in &plan.removals {
            removals.insert(removal.name.clone(), removal.clone());
        }
        for replacement in &plan.replacements {
            replacements.insert(replacement.removed_name.clone(), replacement.clone());
        }
        for transition in &plan.transitions {
            transitions.insert(transition.name.clone(), transition.clone());
        }
        for substitution in &plan.provider_substitutions {
            provider_substitutions.insert(
                (substitution.capability.clone(), substitution.provider.clone()),
                substitution.clone(),
            );
        }
        for conflict in &plan.conflicts {
            conflicts.insert(
                (conflict.selected.clone(), conflict.conflicts_with.clone()),
                conflict.clone(),
            );
        }
    }

    let packages = packages.into_values().collect::<Vec<_>>();
    let removals = removals.into_values().collect::<Vec<_>>();
    let replacements = replacements.into_values().collect::<Vec<_>>();
    let transitions = transitions.into_values().collect::<Vec<_>>();
    let risk_flags = install_plan_risk_flags(&packages, &removals, &replacements, &transitions);

    InstallPlan {
        operation,
        target,
        packages,
        removals,
        replacements,
        transitions,
        provider_substitutions: provider_substitutions.into_values().collect(),
        conflicts: conflicts.into_values().collect(),
        risk_flags,
    }
}

fn install_plan_risk_flags(
    packages: &[InstallPlanPackage],
    removals: &[InstallPlanRemoval],
    replacements: &[InstallPlanReplacement],
    transitions: &[InstallPlanTransition],
) -> Vec<String> {
    let mut risk_flags = BTreeSet::new();
    if !packages.is_empty() {
        risk_flags.insert("adds".to_string());
    }
    if !removals.is_empty() {
        risk_flags.insert("removals".to_string());
    }
    if !replacements.is_empty() {
        risk_flags.insert("replacements".to_string());
    }
    if !transitions.is_empty() {
        risk_flags.insert("version-transitions".to_string());
    }
    let mut mutating_packages = packages
        .iter()
        .map(|package| package.name.clone())
        .chain(transitions.iter().map(|transition| transition.name.clone()))
        .collect::<BTreeSet<_>>();
    mutating_packages.extend(
        replacements
            .iter()
            .map(|replacement| replacement.replacement_name.clone()),
    );
    if mutating_packages.len() > 1 {
        risk_flags.insert("multi-package-transaction".to_string());
    }
    if risk_flags.is_empty() {
        risk_flags.insert("none".to_string());
    }
    risk_flags.into_iter().collect()
}

fn install_plan_operation_name(operation: &PlanOperation) -> &'static str {
    match operation {
        PlanOperation::Install => "install",
        PlanOperation::Upgrade => "upgrade",
        PlanOperation::Uninstall => "uninstall",
        PlanOperation::BundleApply => "bundle-apply",
    }
}

#[cfg(test)]
fn install_plan_from_transaction_preview(
    operation: PlanOperation,
    target: Option<String>,
    preview: &TransactionPreview,
) -> InstallPlan {
    InstallPlan {
        operation,
        target,
        packages: preview
            .adds
            .iter()
            .map(|add| InstallPlanPackage {
                name: add.name.clone(),
                version: add.version.clone(),
                target: add.target.clone(),
                install_reason: "add".to_string(),
                dependencies: Vec::new(),
            })
            .collect(),
        removals: preview
            .removals
            .iter()
            .map(|removal| InstallPlanRemoval {
                name: removal.name.clone(),
                version: removal.version.clone(),
                reason: "replacement".to_string(),
            })
            .collect(),
        replacements: preview
            .replacements
            .iter()
            .map(|replacement| InstallPlanReplacement {
                removed_name: replacement.from_name.clone(),
                removed_version: replacement.from_version.clone(),
                replacement_name: replacement.to_name.clone(),
                replacement_version: replacement.to_version.clone(),
                requirement: String::new(),
            })
            .collect(),
        transitions: preview
            .transitions
            .iter()
            .map(|transition| InstallPlanTransition {
                name: transition.name.clone(),
                from_version: transition.from_version.clone(),
                to_version: transition.to_version.clone(),
            })
            .collect(),
        provider_substitutions: Vec::new(),
        conflicts: Vec::new(),
        risk_flags: preview.risk_flags.clone(),
    }
}

fn render_transaction_preview_lines(
    preview: &TransactionPreview,
    mode: TransactionPreviewMode,
) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!(
        "transaction_preview operation={} mode={}",
        preview.operation,
        mode.as_str()
    ));
    lines.push(format!(
        "transaction_summary adds={} removals={} replacements={} transitions={}",
        preview.adds.len(),
        preview.removals.len(),
        preview.replacements.len(),
        preview.transitions.len()
    ));
    lines.push(format!("risk_flags={}", preview.risk_flags.join(",")));

    for add in &preview.adds {
        lines.push(format!(
            "change_add name={} version={} target={}",
            add.name, add.version, add.target
        ));
    }
    for removal in &preview.removals {
        lines.push(format!(
            "change_remove name={} version={} reason=replacement",
            removal.name, removal.version
        ));
    }
    for replacement in &preview.replacements {
        lines.push(format!(
            "change_replace from={}@{} to={}@{}",
            replacement.from_name,
            replacement.from_version,
            replacement.to_name,
            replacement.to_version
        ));
    }
    for transition in &preview.transitions {
        lines.push(format!(
            "change_transition name={} from={} to={}",
            transition.name, transition.from_version, transition.to_version
        ));
    }

    lines
}

fn render_install_plan_preview_lines(
    plan: &InstallPlan,
    mode: TransactionPreviewMode,
    explainability: Option<&DependencyPolicyExplainability>,
) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!(
        "transaction_preview operation={} mode={}",
        install_plan_operation_name(&plan.operation),
        mode.as_str()
    ));
    lines.push(format!(
        "transaction_summary adds={} removals={} replacements={} transitions={}",
        plan.packages.len(),
        plan.removals.len(),
        plan.replacements.len(),
        plan.transitions.len()
    ));
    lines.push(format!("risk_flags={}", plan.risk_flags.join(",")));

    for package in &plan.packages {
        lines.push(format!(
            "change_add name={} version={} target={}",
            package.name, package.version, package.target
        ));
    }
    for removal in &plan.removals {
        lines.push(format!(
            "change_remove name={} version={} reason={}",
            removal.name, removal.version, removal.reason
        ));
    }
    for replacement in &plan.replacements {
        lines.push(format!(
            "change_replace from={}@{} to={}@{}",
            replacement.removed_name,
            replacement.removed_version,
            replacement.replacement_name,
            replacement.replacement_version
        ));
    }
    for transition in &plan.transitions {
        lines.push(format!(
            "change_transition name={} from={} to={}",
            transition.name, transition.from_version, transition.to_version
        ));
    }

    if let Some(explainability) = explainability {
        lines.extend(render_dependency_policy_explainability_lines(
            explainability,
        ));
    }

    lines
}

fn build_dependency_policy_explainability(
    resolved: &[ResolvedInstall],
    receipts: &[InstallReceipt],
    roots: &[RootInstallRequest],
) -> Result<DependencyPolicyExplainability> {
    let plan = build_install_plan_from_resolved(
        PlanOperation::Install,
        resolved.first().map(|package| package.resolved_target.clone()),
        resolved,
        receipts,
        roots,
    );
    Ok(dependency_policy_explainability_from_install_plan(&plan))
}

fn dependency_policy_explainability_from_install_plan(
    plan: &InstallPlan,
) -> DependencyPolicyExplainability {
    DependencyPolicyExplainability {
        provider_substitutions: plan
            .provider_substitutions
            .iter()
            .map(|substitution| PolicyProviderSubstitution {
                capability: substitution.capability.clone(),
                selected_package: substitution.provider.clone(),
                selected_version: substitution.provider_version.clone(),
            })
            .collect(),
        replacement_removals: plan
            .replacements
            .iter()
            .map(|replacement| PolicyReplacementRemoval {
                selected_package: replacement.replacement_name.clone(),
                selected_version: replacement.replacement_version.clone(),
                removed_package: replacement.removed_name.clone(),
                removed_version: replacement.removed_version.clone(),
                replacement_requirement: replacement.requirement.clone(),
            })
            .collect(),
        conflict_constraints: plan
            .conflicts
            .iter()
            .map(|conflict| PolicyConflictConstraint {
                selected_package: conflict.selected.clone(),
                selected_version: conflict.selected_version.clone(),
                conflict_package: conflict.conflicts_with.clone(),
                conflict_requirement: conflict.requirement.clone(),
            })
            .collect(),
    }
}

fn merge_dependency_policy_explainability(
    destination: &mut DependencyPolicyExplainability,
    mut source: DependencyPolicyExplainability,
) {
    destination
        .provider_substitutions
        .append(&mut source.provider_substitutions);
    destination
        .replacement_removals
        .append(&mut source.replacement_removals);
    destination
        .conflict_constraints
        .append(&mut source.conflict_constraints);
}

fn render_dependency_policy_explainability_lines(
    explainability: &DependencyPolicyExplainability,
) -> Vec<String> {
    let mut lines = Vec::new();

    for substitution in &explainability.provider_substitutions {
        lines.push(format!(
            "explain_provider capability={} selected={}@{}",
            substitution.capability, substitution.selected_package, substitution.selected_version
        ));
    }

    for replacement in &explainability.replacement_removals {
        lines.push(format!(
            "explain_replacement selected={}@{} removes={}@{} declared={}",
            replacement.selected_package,
            replacement.selected_version,
            replacement.removed_package,
            replacement.removed_version,
            replacement.replacement_requirement
        ));
    }

    for conflict in &explainability.conflict_constraints {
        lines.push(format!(
            "explain_conflict selected={}@{} conflicts={}({})",
            conflict.selected_package,
            conflict.selected_version,
            conflict.conflict_package,
            conflict.conflict_requirement
        ));
    }

    lines
}

fn render_dry_run_output_lines(
    preview: &TransactionPreview,
    mode: TransactionPreviewMode,
    explainability: Option<&DependencyPolicyExplainability>,
) -> Vec<String> {
    let mut lines = render_transaction_preview_lines(preview, mode);
    if let Some(explainability) = explainability {
        lines.extend(render_dependency_policy_explainability_lines(
            explainability,
        ));
    }
    lines
}

fn validate_install_preflight_for_resolved(
    layout: &PrefixLayout,
    resolved: &ResolvedInstall,
    receipts: &[InstallReceipt],
) -> Result<()> {
    let replacement_receipts = collect_replacement_receipts(&resolved.manifest, receipts)?;
    let replacement_targets = replacement_receipts
        .iter()
        .map(|receipt| receipt.name.as_str())
        .collect::<HashSet<_>>();

    let exposed_bins = collect_declared_binaries(&resolved.artifact)?;
    let declared_completions = collect_declared_completions(&resolved.artifact)?;
    let declared_gui_assets =
        collect_declared_gui_assets(&resolved.manifest.name, &resolved.artifact)?;
    let projected_completion_paths = declared_completions
        .iter()
        .map(|completion| {
            projected_exposed_completion_path(
                &resolved.manifest.name,
                completion.shell,
                &completion.path,
            )
        })
        .collect::<Result<Vec<_>>>()?;

    validate_binary_preflight(
        layout,
        &resolved.manifest.name,
        &exposed_bins,
        receipts,
        &replacement_targets,
    )?;
    validate_completion_preflight(
        layout,
        &resolved.manifest.name,
        &projected_completion_paths,
        receipts,
    )?;
    validate_gui_preflight(
        layout,
        &resolved.manifest.name,
        &declared_gui_assets,
        &replacement_targets,
    )?;

    Ok(())
}

fn native_gui_registration_cleanup_kind(kind: &str) -> &str {
    match kind {
        "applications-symlink" | "applications-bundle-copy" => "applications-path",
        _ => kind,
    }
}

fn native_gui_registration_cleanup_identity(
    record: &GuiNativeRegistrationRecord,
) -> (String, String) {
    (
        native_gui_registration_cleanup_kind(record.kind.as_str()).to_string(),
        record.path.clone(),
    )
}

fn select_stale_native_gui_registration_records(
    previous_records: &[GuiNativeRegistrationRecord],
    current_records: &[GuiNativeRegistrationRecord],
) -> Vec<GuiNativeRegistrationRecord> {
    let current_cleanup_identities = current_records
        .iter()
        .map(native_gui_registration_cleanup_identity)
        .collect::<HashSet<_>>();

    previous_records
        .iter()
        .filter(|record| {
            !current_cleanup_identities.contains(&native_gui_registration_cleanup_identity(record))
        })
        .cloned()
        .collect()
}

fn sync_native_gui_registration_state_best_effort(
    layout: &PrefixLayout,
    package_name: &str,
    install_root: &Path,
    declared_gui_apps: &[ArtifactGuiApp],
) -> Result<(Vec<GuiNativeRegistrationRecord>, Vec<String>)> {
    let previous_records = read_gui_native_state(layout, package_name)?;
    let mut current_records = Vec::new();
    let mut warnings = Vec::new();

    for app in declared_gui_apps {
        let (records, app_warnings) = register_native_gui_app_best_effort(
            package_name,
            app,
            install_root,
            &previous_records,
        )?;
        current_records.extend(records);
        warnings.extend(app_warnings);
    }

    let mut seen = HashSet::new();
    current_records.retain(|record| {
        seen.insert((record.key.clone(), record.kind.clone(), record.path.clone()))
    });

    let stale_records =
        select_stale_native_gui_registration_records(&previous_records, &current_records);
    let mut records_to_persist = current_records.clone();
    if !stale_records.is_empty() {
        let stale_warnings = remove_native_gui_registration_best_effort(&stale_records)?;
        if !stale_warnings.is_empty() {
            records_to_persist.extend(stale_records.iter().cloned());
            let mut seen_records = HashSet::new();
            records_to_persist.retain(|record| {
                seen_records.insert((record.key.clone(), record.kind.clone(), record.path.clone()))
            });
        }
        warnings.extend(stale_warnings);
    }

    write_gui_native_state(layout, package_name, &records_to_persist)?;
    Ok((current_records, warnings))
}

fn sync_integration_projection_state(
    layout: &PrefixLayout,
    package_name: &str,
    identity: &InstalledPackageIdentity,
    install_root: &Path,
    integrations: &[PackageIntegration],
    shell_init: &[crosspack_core::PackageShellInit],
) -> Result<Vec<IntegrationProjection>> {
    let previous_projections = read_identity_integration_state(layout, identity)?;
    let previous_shell_init_projections = read_identity_shell_init_state(layout, identity)?;
    let host_platform = current_host_platform();
    let desired_projections = integrations
        .iter()
        .map(|integration| {
            projected_integrations_for_install_root(package_name, install_root, integration, host_platform).map(|projections| {
                projections
                    .into_iter()
                    .filter(|projection| {
                        projection.kind != "service"
                            || service_projection_matches_host(projection, host_platform)
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let desired_shell_init_projections = shell_init
        .iter()
        .map(|entry| projected_shell_init(package_name, entry))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let all_projection_states = read_all_integration_states(layout)?;
    for desired in &desired_projections {
        for (owner, projections) in &all_projection_states {
            if owner == &identity.state_key() || owner == package_name {
                continue;
            }
            if projections
                .iter()
                .any(|projection| projection.rel_path == desired.rel_path)
            {
                return Err(anyhow!(
                    "integration projection '{}' is already owned by package '{}'",
                    desired.rel_path,
                    owner
                ));
            }
        }
    }
    let all_shell_init_states = read_all_shell_init_states(layout)?;
    for desired in &desired_shell_init_projections {
        for (owner, projections) in &all_shell_init_states {
            if owner == &identity.state_key() || owner == package_name {
                continue;
            }
            if projections
                .iter()
                .any(|projection| projection.rel_path == desired.rel_path)
            {
                return Err(anyhow!(
                    "shell init projection '{}' is already owned by package '{}'",
                    desired.rel_path,
                    owner
                ));
            }
        }
    }

    let mut current_projections = Vec::new();
    let mut current_shell_init_projections = Vec::new();
    for integration in integrations {
        current_projections.extend(expose_integrations_for_host_platform(
            layout,
            install_root,
            package_name,
            integration,
            host_platform,
        )?);
    }
    for shell_init in shell_init {
        current_shell_init_projections.extend(expose_shell_init(layout, package_name, shell_init)?);
    }

    for stale_projection in previous_projections.iter().filter(|old| {
        !current_projections
            .iter()
            .any(|current| current.rel_path == old.rel_path)
    }) {
        remove_exposed_integration(layout, stale_projection)?;
    }
    for stale_projection in previous_shell_init_projections.iter().filter(|old| {
        !current_shell_init_projections
            .iter()
            .any(|current| current.rel_path == old.rel_path)
    }) {
        remove_exposed_shell_init(layout, stale_projection)?;
    }
    write_identity_integration_state(layout, identity, &current_projections)?;
    write_integration_state(layout, package_name, &current_projections)?;
    write_identity_shell_init_state(layout, identity, &current_shell_init_projections)?;
    write_shell_init_state(layout, package_name, &current_shell_init_projections)?;
    Ok(current_projections)
}

fn activate_enabled_services_for_install(
    layout: &PrefixLayout,
    package_name: &str,
    package_state_key: &str,
    host: &HostActivationContext,
    integrations: &[PackageIntegration],
    projections: &[IntegrationProjection],
) -> Result<()> {
    for integration in integrations {
        let PackageIntegration::Service {
            name,
            linux_systemd_user,
            macos_launch_agent,
            windows_service,
            enable,
        } = integration
        else {
            continue;
        };
        if !enable {
            continue;
        }

        let integration_key = format!("service:{name}");
        if !projections
            .iter()
            .any(|projection| projection.key == integration_key)
        {
            return Err(anyhow!(
                "enabled service integration '{integration_key}' was not projected"
            ));
        }

        let metadata = ServiceActivationMetadata {
            name: name.clone(),
            source: linux_systemd_user.clone(),
            macos_launch_agent: macos_launch_agent.clone(),
            windows_service: windows_service.clone(),
        };
        let mut plan = plan_service_activation(host, package_name, &metadata)
            .map_err(|err| anyhow!(err))?;
        plan.package_state_key = package_state_key.to_string();
        if read_integration_activation_state(layout)?.iter().any(|record| {
            record.package_state_key == package_state_key
                && record.package == package_name
                && record.integration_key == plan.integration_key
        }) {
            return Err(anyhow!(
                "service activation failed package={} service={} reason={}",
                package_name,
                name,
                IntegrationReasonCode::HostPathConflict.as_str()
            ));
        }
        let mut executor = SystemActivationCommandExecutor;
        let outcome = apply_service_plan(&mut executor, &plan);
        if outcome.reason_code != IntegrationReasonCode::Ok {
            if let Err(rollback_err) = replay_failed_service_activation_rollback(
                host,
                &outcome.rollback,
            ) {
                return Err(anyhow!(
                    "service activation failed package={} service={} reason={} rollback_failed={}",
                    package_name,
                    name,
                    outcome.reason_code.as_str(),
                    rollback_err
                ));
            }
            return Err(anyhow!(
                "service activation failed package={} service={} reason={}",
                package_name,
                name,
                outcome.reason_code.as_str()
            ));
        }
        let mut records = read_integration_activation_state(layout)?;
        records.push(IntegrationActivationRecord {
            package_state_key: package_state_key.to_string(),
            package: package_name.to_string(),
            integration_key: plan.integration_key,
            kind: plan.kind,
            adapter: plan.adapter,
            scope: plan.scope,
            desired_state: IntegrationDesiredState::Running,
            applied_state: outcome.applied_state,
            host_path: Some(plan.host_path),
            reason_code: outcome.reason_code,
        });
        write_integration_activation_state(layout, &records).map(|_| ())?;
    }
    Ok(())
}

fn replay_failed_service_activation_rollback(
    host: &HostActivationContext,
    rollback: &[ActivationRollbackEntry],
) -> Result<()> {
    if rollback.is_empty() {
        return Ok(());
    }

    let owners = rollback.iter().filter_map(|entry| {
        let owner = match entry.operation {
            ActivationRollbackOperation::RemoveCreatedSymlink
            | ActivationRollbackOperation::RemoveCreatedWindowsShim
            | ActivationRollbackOperation::RemoveCreatedServiceMetadata => entry.created_owner.clone(),
            ActivationRollbackOperation::RestoreOwnedSymlink
            | ActivationRollbackOperation::RestoreOwnedWindowsShim
            | ActivationRollbackOperation::RestoreOwnedServiceMetadata => entry.expected_current_owner.clone(),
        }?;
        Some((entry.path.clone(), owner))
    });
    let mut fs = RealActivationFs::new(host.platform, owners);
    for entry in rollback.iter().rev() {
        let outcome = replay_activation_rollback_entry_with_fs(&mut fs, entry);
        if outcome.reason_code != IntegrationReasonCode::Ok {
            return Err(anyhow!(
                "path={} reason={}",
                entry.path,
                outcome.reason_code.as_str()
            ));
        }
    }
    Ok(())
}

struct InstallResolvedOptions<'a> {
    snapshot_id: Option<&'a str>,
    force_redownload: bool,
    interaction_policy: InstallInteractionPolicy,
    progress_enabled: bool,
}

struct SourceBuildJournal<'a> {
    txid: &'a str,
    seq: &'a mut u64,
}

struct InstallResolvedPlanContext<'a> {
    root_names: &'a [String],
    install_plan: &'a InstallPlan,
    planned_dependency_overrides: &'a HashMap<String, Vec<String>>,
}

struct InstallPlanApplication {
    replacement_receipts: Vec<InstallReceipt>,
    install_reason: InstallReason,
}

fn set_install_progress_phase(
    progress: &mut Option<TerminalProgress>,
    package: &str,
    phase: &str,
    step: usize,
    total_steps: usize,
    download_progress: Option<(u64, Option<u64>)>,
) {
    if let Some(active_progress) = progress.as_mut() {
        active_progress.set_install_phase(package, phase, step, total_steps, download_progress);
    }
}

fn install_plan_application_for_package(
    plan: &InstallPlan,
    package_name: &str,
    receipts: &[InstallReceipt],
    root_names: &[String],
) -> Result<InstallPlanApplication> {
    let replacement_receipts = plan
        .replacements
        .iter()
        .filter(|replacement| replacement.replacement_name == package_name)
        .filter_map(|replacement| {
            receipts
                .iter()
                .find(|receipt| receipt.name == replacement.removed_name)
                .cloned()
        })
        .collect::<Vec<_>>();
    let planned_install_reason = plan
        .packages
        .iter()
        .find(|package| package.name == package_name)
        .map(|package| package.install_reason.as_str());

    let install_reason = if root_names.iter().any(|root| root == package_name)
        || planned_install_reason == Some("root")
        || replacement_receipts
            .iter()
            .any(|receipt| receipt.install_reason == InstallReason::Root)
    {
        InstallReason::Root
    } else if let Some(existing) = receipts.iter().find(|receipt| receipt.name == package_name) {
        existing.install_reason.clone()
    } else {
        InstallReason::Dependency
    };

    Ok(InstallPlanApplication {
        replacement_receipts,
        install_reason,
    })
}

fn install_resolved(
    layout: &PrefixLayout,
    resolved: &ResolvedInstall,
    dependency_receipts: &[String],
    plan_context: InstallResolvedPlanContext<'_>,
    options: InstallResolvedOptions<'_>,
    mut source_build_journal: Option<&mut SourceBuildJournal<'_>>,
) -> Result<InstallOutcome> {
    const INSTALL_PROGRESS_STEPS: usize = 7;
    let output_style = current_output_style();
    let renderer = TerminalRenderer::from_style(output_style);
    let mut progress = options
        .progress_enabled
        .then(|| renderer.start_progress("install", INSTALL_PROGRESS_STEPS as u64));
    set_install_progress_phase(
        &mut progress,
        &resolved.manifest.name,
        "preflight",
        1,
        INSTALL_PROGRESS_STEPS,
        None,
    );

    let receipts = read_install_receipts(layout)?;
    validate_install_preflight_for_resolved(layout, resolved, &receipts)?;
    let plan_application = install_plan_application_for_package(
        plan_context.install_plan,
        &resolved.manifest.name,
        &receipts,
        plan_context.root_names,
    )?;

    let exposed_bins = collect_declared_binaries(&resolved.artifact)?;
    let declared_completions = collect_declared_completions(&resolved.artifact)?;
    let declared_gui_apps = collect_declared_gui_apps(&resolved.artifact)?;

    let download_url = if let Some(source_build) = resolved.source_build.as_ref() {
        source_build.url.as_str()
    } else {
        resolved.artifact.url.as_str()
    };
    let cache_path = resolved_artifact_cache_path(
        layout,
        &resolved.manifest.name,
        &resolved.manifest.version.to_string(),
        &resolved.resolved_target,
        resolved.archive_type,
        download_url,
    )?;
    set_install_progress_phase(
        &mut progress,
        &resolved.manifest.name,
        "download",
        2,
        INSTALL_PROGRESS_STEPS,
        Some((0, None)),
    );
    let download_status = download_artifact_with_progress(
        download_url,
        &cache_path,
        options.force_redownload,
        |downloaded_bytes, total_bytes| {
            set_install_progress_phase(
                &mut progress,
                &resolved.manifest.name,
                "download",
                2,
                INSTALL_PROGRESS_STEPS,
                Some((downloaded_bytes, total_bytes)),
            );
        },
    )?;

    if let (Some(_source_build), Some(journal)) = (
        resolved.source_build.as_ref(),
        source_build_journal.as_deref_mut(),
    ) {
        append_source_build_journal_entry(
            layout,
            journal,
            format!("source_fetch:{}", resolved.manifest.name),
            Some(cache_path.display().to_string()),
        )?;
    }

    set_install_progress_phase(
        &mut progress,
        &resolved.manifest.name,
        "verify",
        3,
        INSTALL_PROGRESS_STEPS,
        None,
    );
    let (expected_sha256, checksum_kind) =
        if let Some(source_build) = resolved.source_build.as_ref() {
            (source_build.archive_sha256.as_str(), "source archive")
        } else {
            (resolved.artifact.sha256.as_str(), "artifact")
        };
    let checksum_ok = verify_sha256_file(&cache_path, expected_sha256)?;
    if !checksum_ok {
        let _ = remove_file_if_exists(&cache_path);
        return Err(anyhow!(
            "{checksum_kind} sha256 mismatch for {} (expected {})",
            cache_path.display(),
            expected_sha256
        ));
    }

    let identity = InstalledPackageIdentity {
        profile: "default".to_string(),
        target: Some(resolved.resolved_target.clone()),
        source_namespace: "default".to_string(),
        source_provenance: Some("unknown".to_string()),
        package: resolved.manifest.name.clone(),
    };
    let identity_package_dir =
        layout.identity_package_dir(&identity, &resolved.manifest.version.to_string());

    set_install_progress_phase(
        &mut progress,
        &resolved.manifest.name,
        "install",
        4,
        INSTALL_PROGRESS_STEPS,
        None,
    );
    let (install_root, selected_install_mode) = if let Some(source_build) =
        resolved.source_build.as_ref()
    {
        let install_root = install_from_source_archive_to_dir(
            layout,
            &identity_package_dir,
            &cache_path,
            source_build.archive_type,
            &source_build.build_commands,
            &source_build.install_commands,
        )?;
        if let Some(journal) = source_build_journal.as_mut() {
            append_source_build_journal_entry(
                layout,
                journal,
                format!(
                    "source_build_system:{}:{}",
                    resolved.manifest.name, source_build.build_system
                ),
                None,
            )?;
            append_source_build_journal_entry(
                layout,
                journal,
                format!("source_install:{}", resolved.manifest.name),
                Some(install_root.display().to_string()),
            )?;
        }
        (install_root, InstallMode::Managed)
    } else {
        let install_options = build_artifact_install_options(resolved, options.interaction_policy);
        let install_root = install_from_artifact_to_dir(
            layout,
            &identity_package_dir,
            &cache_path,
            resolved.archive_type,
            install_options,
        )?;
        (install_root, install_options.install_mode)
    };

    if let Err(err) =
        apply_replacement_handoff(
            layout,
            &plan_application.replacement_receipts,
            plan_context.planned_dependency_overrides,
        )
    {
        let _ = std::fs::remove_dir_all(&install_root);
        return Err(err);
    }

    let receipts = read_install_receipts(layout)?;

    set_install_progress_phase(
        &mut progress,
        &resolved.manifest.name,
        "expose",
        5,
        INSTALL_PROGRESS_STEPS,
        None,
    );
    for binary in &resolved.artifact.binaries {
        expose_binary(layout, &install_root, &binary.name, &binary.path)?;
    }

    let mut exposed_completions = Vec::with_capacity(declared_completions.len());
    for completion in &declared_completions {
        let storage_path = expose_completion(
            layout,
            &install_root,
            &resolved.manifest.name,
            completion.shell,
            &completion.path,
        )?;
        exposed_completions.push(storage_path);
    }

    let mut exposed_gui_assets = Vec::new();
    for app in &declared_gui_apps {
        let exposed = expose_gui_app(layout, &install_root, &resolved.manifest.name, app)?;
        exposed_gui_assets.extend(exposed);
    }

    if let Some(previous_receipt) = receipts
        .iter()
        .find(|receipt| receipt.name == resolved.manifest.name)
    {
        for stale_bin in previous_receipt
            .exposed_bins
            .iter()
            .filter(|old| !exposed_bins.contains(old))
        {
            remove_exposed_binary(layout, stale_bin)?;
        }
        for stale_completion in previous_receipt
            .exposed_completions
            .iter()
            .filter(|old| !exposed_completions.contains(old))
        {
            remove_exposed_completion(layout, stale_completion)?;
        }
    }

    let previous_gui_assets = read_gui_exposure_state(layout, &resolved.manifest.name)?;
    for stale_gui_asset in previous_gui_assets.iter().filter(|old| {
        !exposed_gui_assets
            .iter()
            .any(|current| current.rel_path == old.rel_path)
    }) {
        remove_exposed_gui_asset(layout, stale_gui_asset)?;
    }
    write_identity_gui_exposure_state(layout, &identity, &exposed_gui_assets)?;

    let exposed_integrations = sync_integration_projection_state(
        layout,
        &resolved.manifest.name,
        &identity,
        &install_root,
        &resolved.manifest.integrations,
        &resolved.manifest.shell_init,
    )?;
    let host = current_host_activation_context(layout)?;
    activate_enabled_services_for_install(
        layout,
        &resolved.manifest.name,
        &identity.state_key(),
        &host,
        &resolved.manifest.integrations,
        &exposed_integrations,
    )?;

    let (native_gui_records, native_gui_warnings) = sync_native_gui_registration_state_best_effort(
        layout,
        &resolved.manifest.name,
        &install_root,
        &declared_gui_apps,
    )?;

    set_install_progress_phase(
        &mut progress,
        &resolved.manifest.name,
        "receipt",
        6,
        INSTALL_PROGRESS_STEPS,
        None,
    );
    let receipt = InstallReceipt {
        name: resolved.manifest.name.clone(),
        version: resolved.manifest.version.to_string(),
        dependencies: dependency_receipts.to_vec(),
        target: Some(resolved.resolved_target.clone()),
        artifact_url: Some(download_url.to_string()),
        artifact_sha256: Some(
            resolved
                .source_build
                .as_ref()
                .map(|plan| plan.archive_sha256.clone())
                .unwrap_or_else(|| resolved.artifact.sha256.clone()),
        ),
        cache_path: Some(cache_path.display().to_string()),
        exposed_bins: exposed_bins.clone(),
        exposed_completions: exposed_completions.clone(),
        snapshot_id: options.snapshot_id.map(ToOwned::to_owned),
        install_mode: selected_install_mode,
        install_reason: plan_application.install_reason,
        install_status: "installed".to_string(),
        installed_at_unix: current_unix_timestamp()?,
    };
    write_identity_declared_services_state(layout, &identity, &resolved.manifest.services)?;
    write_declared_services_state(layout, &resolved.manifest.name, &resolved.manifest.services)?;
    write_identity_gui_native_state(layout, &identity, &native_gui_records)?;
    write_identity_integration_state(layout, &identity, &exposed_integrations)?;
    write_install_receipt(layout, &receipt)?;
    let receipt_path = write_identity_install_receipt(layout, &identity, &receipt)?;
    write_installed_package_state(
        layout,
        &InstalledPackageState {
            identity,
            version: receipt.version.clone(),
            receipt: receipt.clone(),
            gui_assets: exposed_gui_assets.clone(),
            native_gui_records: native_gui_records.clone(),
            services: resolved.manifest.services.clone(),
            integrations: exposed_integrations.clone(),
        },
    )?;
    set_install_progress_phase(
        &mut progress,
        &resolved.manifest.name,
        "complete",
        7,
        INSTALL_PROGRESS_STEPS,
        None,
    );
    finish_progress(progress);

    Ok(InstallOutcome {
        name: resolved.manifest.name.clone(),
        version: resolved.manifest.version.to_string(),
        resolved_target: resolved.resolved_target.clone(),
        archive_type: resolved.archive_type,
        artifact_url: download_url.to_string(),
        cache_path,
        download_status,
        install_root,
        receipt_path,
        exposed_bins,
        exposed_completions,
        exposed_gui_assets: exposed_gui_assets
            .iter()
            .map(|asset| asset.key.clone())
            .collect(),
        exposed_integrations: exposed_integrations
            .iter()
            .map(|projection| projection.key.clone())
            .collect(),
        native_gui_records: native_gui_records
            .iter()
            .map(|record| record.key.clone())
            .collect(),
        warnings: native_gui_warnings,
    })
}

fn append_source_build_journal_entry(
    layout: &PrefixLayout,
    journal: &mut SourceBuildJournal<'_>,
    step: String,
    path: Option<String>,
) -> Result<()> {
    append_transaction_journal_entry(
        layout,
        journal.txid,
        &TransactionJournalEntry {
            seq: *journal.seq,
            step,
            state: "done".to_string(),
            path,
        },
    )?;
    *journal.seq += 1;
    Ok(())
}

fn is_valid_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.as_bytes().iter().all(u8::is_ascii_hexdigit)
}

fn resolved_artifact_cache_path(
    layout: &PrefixLayout,
    package_name: &str,
    version: &str,
    target: &str,
    archive_type: ArchiveType,
    artifact_url: &str,
) -> Result<PathBuf> {
    let mut cache_path = layout.artifact_cache_path(package_name, version, target, archive_type);
    if archive_type == ArchiveType::Bin {
        cache_path.set_file_name(bin_cache_file_name_from_url(artifact_url)?);
    }
    Ok(cache_path)
}

fn bin_cache_file_name_from_url(artifact_url: &str) -> Result<String> {
    let without_fragment = artifact_url.split('#').next().unwrap_or(artifact_url);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    let file_name = without_query.rsplit('/').next().unwrap_or("");

    if file_name.is_empty() || file_name == "." || file_name == ".." || file_name.contains('\\') {
        return Err(anyhow!(
            "could not infer bin cache file name from URL '{artifact_url}'"
        ));
    }

    Ok(file_name.to_string())
}

fn format_install_outcome_lines(outcome: &InstallOutcome, style: OutputStyle) -> Vec<String> {
    let detail_style = if style == OutputStyle::Rich {
        OutputStyle::Plain
    } else {
        style
    };

    let mut lines = vec![
        render_status_line(
            detail_style,
            "ok",
            &format!(
                "resolved {} {} for {}",
                outcome.name, outcome.version, outcome.resolved_target
            ),
        ),
        render_status_line(
            detail_style,
            "step",
            &format!("archive: {}", outcome.archive_type.as_str()),
        ),
        render_status_line(
            detail_style,
            "step",
            &format!("artifact: {}", outcome.artifact_url),
        ),
        render_status_line(
            detail_style,
            "step",
            &format!(
                "cache: {} ({})",
                outcome.cache_path.display(),
                outcome.download_status
            ),
        ),
        render_status_line(
            detail_style,
            "step",
            &format!("install_root: {}", outcome.install_root.display()),
        ),
    ];

    if !outcome.exposed_bins.is_empty() {
        lines.push(render_status_line(
            detail_style,
            "step",
            &format!("exposed_bins: {}", outcome.exposed_bins.join(", ")),
        ));
    }
    if !outcome.exposed_completions.is_empty() {
        lines.push(render_status_line(
            detail_style,
            "step",
            &format!(
                "exposed_completions: {}",
                outcome.exposed_completions.join(", ")
            ),
        ));
    }
    if !outcome.exposed_gui_assets.is_empty() {
        lines.push(render_status_line(
            detail_style,
            "step",
            &format!(
                "exposed_gui_assets: {}",
                outcome.exposed_gui_assets.join(", ")
            ),
        ));
    }
    if !outcome.exposed_integrations.is_empty() {
        lines.push(render_status_line(
            detail_style,
            "step",
            &format!(
                "exposed_integrations: {}",
                outcome.exposed_integrations.join(", ")
            ),
        ));
    }
    if !outcome.native_gui_records.is_empty() {
        lines.push(render_status_line(
            detail_style,
            "step",
            &format!(
                "native_gui_records: {}",
                outcome.native_gui_records.join(", ")
            ),
        ));
    }
    for warning in &outcome.warnings {
        lines.push(render_status_line(
            detail_style,
            "warn",
            &format!("warning: {warning}"),
        ));
    }
    lines.push(render_status_line(
        detail_style,
        "step",
        &format!("receipt: {}", outcome.receipt_path.display()),
    ));

    lines
}

fn format_rich_install_outcome_lines(outcome: &InstallOutcome) -> Vec<String> {
    let mut lines = vec![
        render_rich_install_detail_row(
            "ok",
            "resolved",
            &format!(
                "{} {} for {}",
                outcome.name, outcome.version, outcome.resolved_target
            ),
        ),
        render_rich_install_detail_row("step", "archive", outcome.archive_type.as_str()),
        render_rich_install_detail_row("step", "artifact", &outcome.artifact_url),
        render_rich_install_detail_row(
            "step",
            "cache",
            &format!(
                "{} ({})",
                outcome.cache_path.display(),
                outcome.download_status
            ),
        ),
        render_rich_install_detail_row(
            "step",
            "install_root",
            &outcome.install_root.display().to_string(),
        ),
    ];

    if !outcome.exposed_bins.is_empty() {
        lines.push(render_rich_install_detail_row(
            "step",
            "exposed_bins",
            &outcome.exposed_bins.join(", "),
        ));
    }
    if !outcome.exposed_completions.is_empty() {
        lines.push(render_rich_install_detail_row(
            "step",
            "exposed_completions",
            &outcome.exposed_completions.join(", "),
        ));
    }
    if !outcome.exposed_gui_assets.is_empty() {
        lines.push(render_rich_install_detail_row(
            "step",
            "exposed_gui_assets",
            &outcome.exposed_gui_assets.join(", "),
        ));
    }
    if !outcome.exposed_integrations.is_empty() {
        lines.push(render_rich_install_detail_row(
            "step",
            "exposed_integrations",
            &outcome.exposed_integrations.join(", "),
        ));
    }
    if !outcome.native_gui_records.is_empty() {
        lines.push(render_rich_install_detail_row(
            "step",
            "native_gui_records",
            &outcome.native_gui_records.join(", "),
        ));
    }
    for warning in &outcome.warnings {
        lines.push(render_rich_install_detail_row("warn", "warning", warning));
    }
    lines.push(render_rich_install_detail_row(
        "step",
        "receipt",
        &outcome.receipt_path.display().to_string(),
    ));

    lines
}

fn print_install_outcome(outcome: &InstallOutcome, style: OutputStyle) {
    let renderer = TerminalRenderer::from_style(style);
    renderer.print_section(&format!("Installed {} {}", outcome.name, outcome.version));
    let lines = match style {
        OutputStyle::Plain => format_install_outcome_lines(outcome, OutputStyle::Plain),
        OutputStyle::Rich => format_rich_install_outcome_lines(outcome),
    };
    renderer.print_lines(&lines);
}

fn collect_declared_binaries(artifact: &Artifact) -> Result<Vec<String>> {
    let mut names = Vec::with_capacity(artifact.binaries.len());
    let mut seen = HashSet::new();
    for binary in &artifact.binaries {
        validate_binary_name(&binary.name)?;
        if !seen.insert(binary.name.clone()) {
            return Err(anyhow!(
                "duplicate binary declaration '{}' for target '{}'",
                binary.name,
                artifact.target
            ));
        }
        names.push(binary.name.clone());
    }
    Ok(names)
}

#[derive(Debug, Clone)]
struct DeclaredCompletion {
    shell: ArtifactCompletionShell,
    path: String,
}

fn collect_declared_completions(artifact: &Artifact) -> Result<Vec<DeclaredCompletion>> {
    let mut declared = Vec::with_capacity(artifact.completions.len());
    let mut seen = HashSet::new();
    for completion in &artifact.completions {
        let key = (completion.shell, completion.path.clone());
        if !seen.insert(key) {
            return Err(anyhow!(
                "duplicate completion declaration for shell '{}' and path '{}' in target '{}'",
                completion.shell.as_str(),
                completion.path,
                artifact.target
            ));
        }
        declared.push(DeclaredCompletion {
            shell: completion.shell,
            path: completion.path.clone(),
        });
    }
    Ok(declared)
}

fn collect_declared_gui_apps(artifact: &Artifact) -> Result<Vec<ArtifactGuiApp>> {
    let mut declared = Vec::with_capacity(artifact.gui_apps.len());
    let mut seen = HashSet::new();
    for app in &artifact.gui_apps {
        if !seen.insert(app.app_id.clone()) {
            return Err(anyhow!(
                "duplicate gui app declaration '{}' for target '{}'",
                app.app_id,
                artifact.target
            ));
        }
        declared.push(app.clone());
    }
    Ok(declared)
}

fn collect_declared_gui_assets(
    package_name: &str,
    artifact: &Artifact,
) -> Result<Vec<GuiExposureAsset>> {
    let declared_apps = collect_declared_gui_apps(artifact)?;
    let mut assets = Vec::new();
    let mut seen_keys = HashSet::new();
    let mut seen_paths = HashMap::new();
    for app in &declared_apps {
        let projected = projected_gui_assets(package_name, app)?;
        let projected_paths = projected
            .iter()
            .map(|asset| asset.rel_path.clone())
            .collect::<HashSet<_>>();
        for rel_path in projected_paths {
            if let Some(existing_app_id) =
                seen_paths.insert(rel_path.clone(), app.app_id.trim().to_ascii_lowercase())
            {
                return Err(anyhow!(
                    "duplicate gui storage path declaration '{}' for package '{}' target '{}'; app '{}' collides with app '{}'",
                    rel_path,
                    package_name,
                    artifact.target,
                    app.app_id,
                    existing_app_id
                ));
            }
        }
        for asset in projected {
            if !seen_keys.insert(asset.key.clone()) {
                return Err(anyhow!(
                    "duplicate gui ownership key declaration '{}' for package '{}' target '{}'",
                    asset.key,
                    package_name,
                    artifact.target
                ));
            }
            assets.push(asset);
        }
    }
    Ok(assets)
}

fn validate_binary_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(anyhow!("binary name must not be empty"));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(anyhow!(
            "binary name must not contain path separators: {name}"
        ));
    }
    Ok(())
}

fn validate_completion_preflight(
    layout: &PrefixLayout,
    package_name: &str,
    desired_completion_paths: &[String],
    receipts: &[InstallReceipt],
) -> Result<()> {
    let owned_by_self: HashSet<&str> = receipts
        .iter()
        .find(|receipt| receipt.name == package_name)
        .map(|receipt| {
            receipt
                .exposed_completions
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();

    for desired in desired_completion_paths {
        for receipt in receipts {
            if receipt.name == package_name {
                continue;
            }
            if receipt
                .exposed_completions
                .iter()
                .any(|owned| owned == desired)
            {
                return Err(anyhow!(
                    "completion '{}' is already owned by package '{}'",
                    desired,
                    receipt.name
                ));
            }
        }

        let path = exposed_completion_path(layout, desired)?;
        if path.exists() && !owned_by_self.contains(desired.as_str()) {
            return Err(anyhow!(
                "completion '{}' at {} already exists and is not managed by crosspack",
                desired,
                path.display()
            ));
        }
    }

    Ok(())
}

fn validate_gui_preflight(
    layout: &PrefixLayout,
    package_name: &str,
    desired_gui_assets: &[GuiExposureAsset],
    replacement_targets: &HashSet<&str>,
) -> Result<()> {
    let states = read_all_gui_exposure_states(layout)?;

    let owned_by_self_paths = states
        .get(package_name)
        .map(|assets| {
            assets
                .iter()
                .map(|asset| asset.rel_path.as_str())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();

    let owned_by_replacement_paths = states
        .iter()
        .filter(|(owner, _)| replacement_targets.contains(owner.as_str()))
        .flat_map(|(_, assets)| assets.iter().map(|asset| asset.rel_path.as_str()))
        .collect::<HashSet<_>>();

    for desired in desired_gui_assets {
        for (owner, assets) in &states {
            if owner == package_name || replacement_targets.contains(owner.as_str()) {
                continue;
            }
            if assets.iter().any(|owned| owned.key == desired.key) {
                return Err(anyhow!(
                    "gui ownership key '{}' is already owned by package '{}'",
                    desired.key,
                    owner
                ));
            }
        }

        let path = gui_asset_path(layout, &desired.rel_path)?;
        if path.exists()
            && !owned_by_self_paths.contains(desired.rel_path.as_str())
            && !owned_by_replacement_paths.contains(desired.rel_path.as_str())
        {
            return Err(anyhow!(
                "gui asset '{}' at {} already exists and is not managed by crosspack",
                desired.rel_path,
                path.display()
            ));
        }
    }

    Ok(())
}

fn collect_replacement_receipts(
    manifest: &PackageManifest,
    receipts: &[InstallReceipt],
) -> Result<Vec<InstallReceipt>> {
    let mut matched = receipts
        .iter()
        .filter_map(|receipt| {
            let requirement = manifest.replaces.get(&receipt.name)?;
            let installed = Version::parse(&receipt.version).ok()?;
            requirement.matches(&installed).then_some(receipt.clone())
        })
        .collect::<Vec<_>>();

    for receipt in receipts {
        if manifest.replaces.contains_key(&receipt.name) {
            Version::parse(&receipt.version).with_context(|| {
                format!(
                    "installed receipt for '{}' has invalid version for replacement preflight: {}",
                    receipt.name, receipt.version
                )
            })?;
        }
    }

    matched.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(matched)
}

fn apply_replacement_handoff(
    layout: &PrefixLayout,
    replacement_receipts: &[InstallReceipt],
    planned_dependency_overrides: &HashMap<String, Vec<String>>,
) -> Result<()> {
    let replacement_root_names = replacement_receipts
        .iter()
        .filter(|receipt| receipt.install_reason == InstallReason::Root)
        .map(|receipt| receipt.name.clone())
        .collect::<HashSet<_>>();

    for replacement in replacement_receipts {
        let blocked_by_roots =
            uninstall_blocked_by_roots_with_dependency_overrides_and_ignored_roots(
                layout,
                &replacement.name,
                planned_dependency_overrides,
                &replacement_root_names,
            )?;
        if !blocked_by_roots.is_empty() {
            return Err(anyhow!(
                "cannot replace '{}' {}: still required by roots {}",
                replacement.name,
                replacement.version,
                blocked_by_roots.join(", ")
            ));
        }
    }

    for replacement in replacement_receipts {
        let result = uninstall_package_with_dependency_overrides_and_ignored_roots(
            layout,
            &replacement.name,
            planned_dependency_overrides,
            &replacement_root_names,
        )?;
        if result.status == UninstallStatus::BlockedByDependents {
            return Err(anyhow!(
                "cannot replace '{}' {}: still required by roots {}",
                replacement.name,
                replacement.version,
                result.blocked_by_roots.join(", ")
            ));
        }
    }

    Ok(())
}

fn validate_binary_preflight(
    layout: &PrefixLayout,
    package_name: &str,
    desired_bins: &[String],
    receipts: &[InstallReceipt],
    replacement_targets: &HashSet<&str>,
) -> Result<()> {
    let current_exe = std::env::current_exe().ok();
    validate_binary_preflight_with_current_exe(
        layout,
        package_name,
        desired_bins,
        receipts,
        replacement_targets,
        current_exe.as_deref(),
    )
}

fn validate_binary_preflight_with_current_exe(
    layout: &PrefixLayout,
    package_name: &str,
    desired_bins: &[String],
    receipts: &[InstallReceipt],
    replacement_targets: &HashSet<&str>,
    current_exe: Option<&Path>,
) -> Result<()> {
    let owned_by_self: HashSet<&str> = receipts
        .iter()
        .find(|receipt| receipt.name == package_name)
        .map(|receipt| receipt.exposed_bins.iter().map(String::as_str).collect())
        .unwrap_or_default();

    let owned_by_replacements: HashSet<&str> = receipts
        .iter()
        .filter(|receipt| replacement_targets.contains(receipt.name.as_str()))
        .flat_map(|receipt| receipt.exposed_bins.iter().map(String::as_str))
        .collect();

    for desired in desired_bins {
        for receipt in receipts {
            if receipt.name == package_name || replacement_targets.contains(receipt.name.as_str()) {
                continue;
            }
            if receipt.exposed_bins.iter().any(|bin| bin == desired) {
                return Err(anyhow!(
                    "binary '{}' is already owned by package '{}'",
                    desired,
                    receipt.name
                ));
            }
        }

        let path = bin_path(layout, desired);
        let allows_self_replace = package_name == "crosspack"
            && desired == "crosspack"
            && current_exe
                .map(|exe| path_matches_current_exe(exe, &path))
                .unwrap_or(false);
        if path.exists()
            && !owned_by_self.contains(desired.as_str())
            && !owned_by_replacements.contains(desired.as_str())
            && !allows_self_replace
        {
            return Err(anyhow!(
                "binary '{}' at {} already exists and is not managed by crosspack",
                desired,
                path.display()
            ));
        }
    }

    Ok(())
}

fn path_matches_current_exe(current_exe: &Path, candidate: &Path) -> bool {
    if current_exe == candidate {
        return true;
    }

    let canonical_current = fs::canonicalize(current_exe);
    let canonical_candidate = fs::canonicalize(candidate);
    match (canonical_current, canonical_candidate) {
        (Ok(current), Ok(candidate)) => current == candidate,
        _ => false,
    }
}

fn build_dependency_receipts(
    resolved: &ResolvedInstall,
    selected: &[ResolvedInstall],
) -> Vec<String> {
    let mut deps = resolved
        .manifest
        .dependencies
        .keys()
        .filter_map(|name| {
            selected
                .iter()
                .find(|candidate| candidate.manifest.name == *name)
                .map(|candidate| {
                    format!("{}@{}", candidate.manifest.name, candidate.manifest.version)
                })
        })
        .collect::<Vec<_>>();
    deps.sort();
    deps
}

fn build_planned_dependency_overrides(
    selected: &[ResolvedInstall],
) -> HashMap<String, Vec<String>> {
    selected
        .iter()
        .map(|package| {
            let mut dependencies = package
                .manifest
                .dependencies
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            dependencies.sort();
            dependencies.dedup();
            (package.manifest.name.clone(), dependencies)
        })
        .collect()
}

#[cfg(test)]
fn determine_install_reason(
    package_name: &str,
    root_names: &[String],
    existing_receipts: &[InstallReceipt],
    replacement_receipts: &[InstallReceipt],
) -> InstallReason {
    if root_names.iter().any(|root| root == package_name) {
        return InstallReason::Root;
    }

    let promotes_from_replacement_root = replacement_receipts
        .iter()
        .any(|receipt| receipt.install_reason == InstallReason::Root);

    if let Some(existing) = existing_receipts
        .iter()
        .find(|receipt| receipt.name == package_name)
    {
        if promotes_from_replacement_root {
            return InstallReason::Root;
        }
        return existing.install_reason.clone();
    }

    if promotes_from_replacement_root {
        return InstallReason::Root;
    }

    InstallReason::Dependency
}

#[cfg(test)]
fn build_upgrade_roots(receipts: &[InstallReceipt]) -> Vec<RootInstallRequest> {
    receipts
        .iter()
        .filter(|receipt| receipt.install_reason == InstallReason::Root)
        .map(|receipt| RootInstallRequest {
            name: receipt.name.clone(),
            requirement: VersionReq::STAR,
        })
        .collect()
}

fn build_upgrade_plans(receipts: &[InstallReceipt]) -> Vec<UpgradePlan> {
    let mut grouped_roots: BTreeMap<Option<String>, Vec<String>> = BTreeMap::new();

    for receipt in receipts {
        if receipt.install_reason != InstallReason::Root {
            continue;
        }
        grouped_roots
            .entry(receipt.target.clone())
            .or_default()
            .push(receipt.name.clone());
    }

    grouped_roots
        .into_iter()
        .map(|(target, mut root_names)| {
            root_names.sort();
            root_names.dedup();

            let roots = root_names
                .iter()
                .map(|name| RootInstallRequest {
                    name: name.clone(),
                    requirement: VersionReq::STAR,
                })
                .collect::<Vec<_>>();

            UpgradePlan {
                target,
                roots,
                root_names,
            }
        })
        .collect()
}

fn enforce_disjoint_multi_target_upgrade(
    resolved_by_target: &[(Option<&str>, Vec<String>)],
) -> Result<()> {
    let mut package_targets = BTreeMap::new();

    for (target, packages) in resolved_by_target {
        let target_name = target.unwrap_or("host-default").to_string();
        for package in packages {
            if let Some(previous_target) =
                package_targets.insert(package.clone(), target_name.clone())
            {
                if previous_target != target_name {
                    return Err(anyhow!(
                        "upgrade cannot safely process package '{}' across multiple targets ({} and {}); install state is currently keyed by package name. Use separate prefixes for cross-target installs.",
                        package,
                        previous_target,
                        target_name
                    ));
                }
            }
        }
    }

    Ok(())
}

fn format_uninstall_messages(result: &UninstallResult) -> Vec<String> {
    let version = result.version.as_deref().unwrap_or("unknown");
    let mut lines = match result.status {
        UninstallStatus::NotInstalled => vec![format!("{} is not installed", result.name)],
        UninstallStatus::Uninstalled => vec![format!("uninstalled {} {}", result.name, version)],
        UninstallStatus::RepairedStaleState => vec![format!(
            "removed stale state for {} {} (package files already missing)",
            result.name, version
        )],
        UninstallStatus::BlockedByDependents => vec![format!(
            "cannot uninstall {} {}: still required by roots {}",
            result.name,
            version,
            result.blocked_by_roots.join(", ")
        )],
    };

    if !result.pruned_dependencies.is_empty() {
        lines.push(format!(
            "pruned orphan dependencies: {}",
            result.pruned_dependencies.join(", ")
        ));
    }

    lines
}

fn enforce_no_downgrades(
    receipts: &[InstallReceipt],
    resolved: &[ResolvedInstall],
    operation: &str,
) -> Result<()> {
    for receipt in receipts {
        let Some(candidate) = resolved
            .iter()
            .find(|entry| entry.manifest.name == receipt.name)
        else {
            continue;
        };

        let current = Version::parse(&receipt.version).with_context(|| {
            format!(
                "installed receipt for '{}' has invalid version: {}",
                receipt.name, receipt.version
            )
        })?;
        if candidate.manifest.version < current {
            return Err(anyhow!(
                "{} would downgrade '{}' from {} to {}; run `crosspack install '{}@={}'` to perform an explicit downgrade",
                operation,
                receipt.name,
                receipt.version,
                candidate.manifest.version,
                receipt.name,
                candidate.manifest.version
            ));
        }
    }
    Ok(())
}

fn host_target_triple() -> &'static str {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "windows") => "x86_64-pc-windows-msvc",
        ("aarch64", "windows") => "aarch64-pc-windows-msvc",
        _ => "unknown-unknown-unknown",
    }
}

fn download_artifact_with_progress<F>(
    url: &str,
    cache_path: &Path,
    force_redownload: bool,
    on_progress: F,
) -> Result<&'static str>
where
    F: FnMut(u64, Option<u64>),
{
    const DOWNLOAD_BACKEND_ENV: &str = "CROSSPACK_DOWNLOAD_BACKEND";

    if cache_path.exists() && !force_redownload {
        return Ok("cache-hit");
    }

    let backend = parse_download_backend_preference(
        std::env::var(DOWNLOAD_BACKEND_ENV).ok().as_deref(),
        DOWNLOAD_BACKEND_ENV,
    )?;

    download_artifact_with_progress_using(
        url,
        cache_path,
        force_redownload,
        backend,
        on_progress,
        download_http_to_path,
        download_http_external_to_path,
    )
}

fn download_artifact_with_progress_using<F, InProcessDownload, ExternalDownload>(
    url: &str,
    cache_path: &Path,
    force_redownload: bool,
    backend: DownloadBackendPreference,
    mut on_progress: F,
    mut in_process_download: InProcessDownload,
    mut external_download: ExternalDownload,
) -> Result<&'static str>
where
    F: FnMut(u64, Option<u64>),
    InProcessDownload: FnMut(&str, &Path, &mut F) -> Result<()>,
    ExternalDownload: FnMut(&str, &Path) -> Result<()>,
{
    if cache_path.exists() && !force_redownload {
        return Ok("cache-hit");
    }

    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create cache dir: {}", parent.display()))?;
    }

    let part_path = cache_path.with_file_name(format!(
        "{}.part",
        cache_path
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("artifact")
    ));

    on_progress(0, None);

    let result = match backend {
        DownloadBackendPreference::External => external_download(url, &part_path),
        DownloadBackendPreference::InProcess => match in_process_download(url, &part_path, &mut on_progress) {
            Ok(()) => Ok(()),
            Err(in_process_err) => external_download(url, &part_path).map_err(|external_err| {
                anyhow!(
                    "download failed for {url} using in-process backend and external fallback: in-process: {in_process_err}; external: {external_err}"
                )
            }),
        },
    };

    if let Err(err) = result {
        let _ = std::fs::remove_file(&part_path);
        return Err(err);
    }

    if cache_path.exists() {
        std::fs::remove_file(cache_path)
            .with_context(|| format!("failed to replace cache file: {}", cache_path.display()))?;
    }
    std::fs::rename(&part_path, cache_path).with_context(|| {
        format!(
            "failed to move downloaded artifact into cache: {}",
            cache_path.display()
        )
    })?;

    Ok("downloaded")
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum DownloadBackendPreference {
    InProcess,
    External,
}

fn parse_download_backend_preference(
    value: Option<&str>,
    env_var_name: &str,
) -> Result<DownloadBackendPreference> {
    let normalized = value.map(str::trim).unwrap_or("");
    if normalized.is_empty() || normalized.eq_ignore_ascii_case("in-process") {
        return Ok(DownloadBackendPreference::InProcess);
    }
    if normalized.eq_ignore_ascii_case("external") {
        return Ok(DownloadBackendPreference::External);
    }

    Err(anyhow!(
        "invalid {} value '{}': expected 'external' or 'in-process'",
        env_var_name,
        normalized
    ))
}

fn download_http_to_path<F>(url: &str, out_path: &Path, on_progress: &mut F) -> Result<()>
where
    F: FnMut(u64, Option<u64>),
{
    const MAX_ATTEMPTS: usize = 3;
    let mut last_error = None;

    for attempt in 1..=MAX_ATTEMPTS {
        let _ = std::fs::remove_file(out_path);
        match download_http_to_path_attempt(url, out_path, on_progress) {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_error = Some(err);
                if attempt == MAX_ATTEMPTS {
                    break;
                }
            }
        }
    }

    let final_error = last_error.unwrap_or_else(|| anyhow!("unknown in-process download failure"));
    Err(anyhow!(
        "download failed for {url} after {MAX_ATTEMPTS} in-process attempts: {final_error}"
    ))
}

fn download_http_to_path_attempt<F>(url: &str, out_path: &Path, on_progress: &mut F) -> Result<()>
where
    F: FnMut(u64, Option<u64>),
{
    const CONNECT_TIMEOUT_SECS: u64 = 10;

    let mut client_builder = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS));
    if let Some(request_timeout_secs) = std::env::var("CROSSPACK_DOWNLOAD_REQUEST_TIMEOUT_SECS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|timeout| *timeout > 0)
    {
        client_builder =
            client_builder.timeout(std::time::Duration::from_secs(request_timeout_secs));
    }

    let client = client_builder
        .build()
        .context("failed to initialize HTTP client")?;
    let mut response = client
        .get(url)
        .send()
        .with_context(|| format!("download failed for {url}"))?
        .error_for_status()
        .with_context(|| format!("download failed for {url}"))?;

    let total_bytes = response.content_length();
    on_progress(0, total_bytes);

    let mut out = std::fs::File::create(out_path).with_context(|| {
        format!(
            "failed to create download part file: {}",
            out_path.display()
        )
    })?;

    let mut downloaded_bytes: u64 = 0;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let bytes_read = response
            .read(&mut buffer)
            .with_context(|| format!("download read failed for {url}"))?;
        if bytes_read == 0 {
            break;
        }

        out.write_all(&buffer[..bytes_read]).with_context(|| {
            format!("failed to write download part file: {}", out_path.display())
        })?;
        downloaded_bytes += bytes_read as u64;
        on_progress(downloaded_bytes, total_bytes);
    }

    Ok(())
}

fn download_http_external_to_path(url: &str, out_path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        if download_with_powershell(url, out_path).is_ok() {
            return Ok(());
        }
    }

    download_with_curl(url, out_path).or_else(|curl_err| {
        download_with_wget(url, out_path).map_err(|wget_err| {
            anyhow!("external download failed for {url}: curl: {curl_err}; wget: {wget_err}")
        })
    })
}

fn download_with_curl(url: &str, out_path: &Path) -> Result<()> {
    let mut command = Command::new("curl");
    command
        .arg("-fL")
        .arg("--retry")
        .arg("2")
        .arg("-o")
        .arg(out_path)
        .arg(url);
    run_download_command(&mut command, "curl download failed")
}

fn download_with_wget(url: &str, out_path: &Path) -> Result<()> {
    let mut command = Command::new("wget");
    command.arg("-O").arg(out_path).arg(url);
    run_download_command(&mut command, "wget download failed")
}

#[cfg(windows)]
fn download_with_powershell(url: &str, out_path: &Path) -> Result<()> {
    let mut command = Command::new("powershell");
    command.arg("-NoProfile").arg("-Command").arg(format!(
        "Invoke-WebRequest -Uri '{}' -OutFile '{}'",
        escape_ps_single_quote(url),
        escape_ps_single_quote_path(out_path)
    ));
    run_download_command(&mut command, "powershell download failed")
}

fn run_download_command(command: &mut Command, context_message: &str) -> Result<()> {
    let output = command
        .output()
        .with_context(|| format!("{context_message}: command failed to start"))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(anyhow!(
        "{context_message}: status={} stdout='{}' stderr='{}'",
        output.status,
        stdout.trim(),
        stderr.trim()
    ))
}

fn escape_ps_single_quote(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(windows)]
fn escape_ps_single_quote_path(path: &Path) -> String {
    let mut os = OsString::new();
    os.push(path.as_os_str());
    os.to_string_lossy().replace('\'', "''")
}
