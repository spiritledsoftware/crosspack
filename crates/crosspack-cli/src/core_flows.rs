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

    let filtered = candidates
        .into_iter()
        .filter(|manifest| {
            manifest.name == *provider_name
                && (manifest.name == requested_name
                    || manifest
                        .provides
                        .iter()
                        .any(|provided| provided == requested_name))
        })
        .collect::<Vec<_>>();

    if filtered.is_empty() {
        return Err(anyhow!(
            "provider override '{}={}' did not match any candidate packages",
            requested_name,
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
    native_sidecar_exists: bool,
}

fn begin_transaction(
    layout: &PrefixLayout,
    operation: &str,
    snapshot_id: Option<&str>,
    started_at_unix: u64,
) -> Result<TransactionMetadata> {
    let txid = format!("tx-{started_at_unix}-{}", std::process::id());
    let metadata = TransactionMetadata {
        version: 1,
        txid,
        operation: operation.to_string(),
        status: "planning".to_string(),
        started_at_unix,
        snapshot_id: snapshot_id.map(ToOwned::to_owned),
    };

    write_transaction_metadata(layout, &metadata)?;
    if let Err(err) = set_active_transaction(layout, &metadata.txid) {
        let _ = remove_file_if_exists(&layout.transaction_metadata_path(&metadata.txid));
        let _ = std::fs::remove_dir_all(layout.transaction_staging_path(&metadata.txid));
        return Err(err);
    }

    Ok(metadata)
}

fn set_transaction_status(layout: &PrefixLayout, txid: &str, status: &str) -> Result<()> {
    update_transaction_status(layout, txid, status)
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
        set_transaction_status(layout, &tx.txid, "applying")?;
        run(&tx)?;
        set_transaction_status(layout, &tx.txid, "committed")?;
        clear_active_transaction(layout)?;
        Ok(())
    })();

    match run_result {
        Ok(()) => Ok(()),
        Err(err) => {
            let current_status = read_transaction_metadata(layout, &tx.txid)
                .ok()
                .flatten()
                .map(|metadata| metadata.status);
            let preserve_recovery_state = current_status
                .as_deref()
                .map(|status| {
                    matches!(
                        status,
                        "rolling_back" | "rolled_back" | "committed" | "failed"
                    )
                })
                .unwrap_or(false);
            if matches!(current_status.as_deref(), Some("rolled_back" | "committed")) {
                let _ = clear_active_transaction(layout);
            }
            if !preserve_recovery_state {
                let _ = set_transaction_status(layout, &tx.txid, "failed");
            }
            Err(err)
        }
    }
}

fn status_allows_stale_marker_cleanup(status: &str) -> bool {
    matches!(status, "committed" | "rolled_back")
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
    let active_txid = match read_active_transaction(layout) {
        Ok(active_txid) => active_txid,
        Err(_) => {
            return Err(anyhow!(
                "transaction state requires repair (reason=active_marker_unreadable path={})",
                layout.transaction_active_path().display()
            ));
        }
    };

    if let Some(txid) = active_txid {
        let metadata = match read_transaction_metadata(layout, &txid) {
            Ok(metadata) => metadata,
            Err(_) => {
                return Err(anyhow!(
                    "transaction {txid} requires repair (reason=metadata_unreadable path={})",
                    layout.transaction_metadata_path(&txid).display()
                ));
            }
        };

        if let Some(metadata) = metadata {
            if status_allows_stale_marker_cleanup(&metadata.status) {
                clear_active_transaction(layout)?;
                return Ok(());
            }
            if metadata.status == "rolling_back" {
                return Err(anyhow!(
                    "transaction {txid} requires repair (reason=rolling_back)"
                ));
            }
            if metadata.status == "failed" {
                return Err(anyhow!(
                    "transaction {txid} requires repair (reason=failed)"
                ));
            }

            return Err(anyhow!(
                "transaction {txid} is active (reason=active_status status={})",
                metadata.status
            ));
        }

        return Err(anyhow!(
            "transaction {txid} requires repair (reason=metadata_missing path={})",
            layout.transaction_metadata_path(&txid).display()
        ));
    }

    Ok(())
}

fn doctor_transaction_health_line(layout: &PrefixLayout) -> Result<String> {
    let active_txid = match read_active_transaction(layout) {
        Ok(active_txid) => active_txid,
        Err(_) => {
            return Ok(format!(
                "transaction: failed (reason=active_marker_unreadable path={})",
                layout.transaction_active_path().display()
            ));
        }
    };

    let Some(txid) = active_txid else {
        return Ok("transaction: clean".to_string());
    };

    let metadata = match read_transaction_metadata(layout, &txid) {
        Ok(metadata) => metadata,
        Err(_) => {
            return Ok(format!(
                "transaction: failed {txid} (reason=metadata_unreadable path={})",
                layout.transaction_metadata_path(&txid).display()
            ));
        }
    };

    let Some(metadata) = metadata else {
        return Ok(format!(
            "transaction: failed {txid} (reason=metadata_missing path={})",
            layout.transaction_metadata_path(&txid).display()
        ));
    };

    if metadata.status == "rolling_back" {
        return Ok(format!("transaction: failed {txid} (reason=rolling_back)"));
    }
    if metadata.status == "failed" {
        return Ok(format!("transaction: failed {txid} (reason=failed)"));
    }
    if status_allows_stale_marker_cleanup(&metadata.status) {
        clear_active_transaction(layout)?;
        return Ok("transaction: clean".to_string());
    }

    Ok(format!("transaction: active {txid}"))
}

fn resolve_install_graph(
    layout: &PrefixLayout,
    index: &MetadataBackend,
    roots: &[RootInstallRequest],
    requested_target: Option<&str>,
    provider_overrides: &BTreeMap<String, String>,
) -> Result<Vec<ResolvedInstall>> {
    let (resolved, _) = resolve_install_graph_with_tokens(
        layout,
        index,
        roots,
        requested_target,
        provider_overrides,
        true,
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

    let graph = resolve_dependency_graph(&root_reqs, &pins, |package_name| {
        let versions = index.package_versions(package_name)?;
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

            let artifact = manifest
                .artifacts
                .iter()
                .find(|artifact| artifact.target == resolved_target)
                .ok_or_else(|| {
                    anyhow!(
                        "no artifact available for target {} in {} {}",
                        resolved_target,
                        manifest.name,
                        manifest.version
                    )
                })?
                .clone();
            let archive_type = artifact.archive_type()?;

            Ok(ResolvedInstall {
                manifest,
                artifact,
                resolved_target: resolved_target.clone(),
                archive_type,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok((resolved, resolved_dependency_tokens))
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

#[derive(Clone, Copy)]
struct InstallResolvedOptions<'a> {
    snapshot_id: Option<&'a str>,
    force_redownload: bool,
    interaction_policy: InstallInteractionPolicy,
}

fn install_resolved(
    layout: &PrefixLayout,
    resolved: &ResolvedInstall,
    dependency_receipts: &[String],
    root_names: &[String],
    planned_dependency_overrides: &HashMap<String, Vec<String>>,
    options: InstallResolvedOptions<'_>,
) -> Result<InstallOutcome> {
    let receipts = read_install_receipts(layout)?;
    validate_install_preflight_for_resolved(layout, resolved, &receipts)?;

    let replacement_receipts = collect_replacement_receipts(&resolved.manifest, &receipts)?;

    let exposed_bins = collect_declared_binaries(&resolved.artifact)?;
    let declared_completions = collect_declared_completions(&resolved.artifact)?;
    let declared_gui_apps = collect_declared_gui_apps(&resolved.artifact)?;

    let cache_path = resolved_artifact_cache_path(
        layout,
        &resolved.manifest.name,
        &resolved.manifest.version.to_string(),
        &resolved.resolved_target,
        resolved.archive_type,
        &resolved.artifact.url,
    )?;
    let download_status = download_artifact(
        &resolved.artifact.url,
        &cache_path,
        options.force_redownload,
    )?;

    let checksum_ok = verify_sha256_file(&cache_path, &resolved.artifact.sha256)?;
    if !checksum_ok {
        let _ = remove_file_if_exists(&cache_path);
        return Err(anyhow!(
            "sha256 mismatch for {} (expected {})",
            cache_path.display(),
            resolved.artifact.sha256
        ));
    }

    let install_options = build_artifact_install_options(resolved, options.interaction_policy);
    let selected_install_mode = install_options.install_mode;
    let install_root = install_from_artifact(
        layout,
        &resolved.manifest.name,
        &resolved.manifest.version.to_string(),
        &cache_path,
        resolved.archive_type,
        install_options,
    )?;

    if let Err(err) =
        apply_replacement_handoff(layout, &replacement_receipts, planned_dependency_overrides)
    {
        let _ = std::fs::remove_dir_all(&install_root);
        return Err(err);
    }

    let receipts = read_install_receipts(layout)?;

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
    write_gui_exposure_state(layout, &resolved.manifest.name, &exposed_gui_assets)?;

    let (native_gui_records, native_gui_warnings) = sync_native_gui_registration_state_best_effort(
        layout,
        &resolved.manifest.name,
        &install_root,
        &declared_gui_apps,
    )?;

    let receipt = InstallReceipt {
        name: resolved.manifest.name.clone(),
        version: resolved.manifest.version.to_string(),
        dependencies: dependency_receipts.to_vec(),
        target: Some(resolved.resolved_target.clone()),
        artifact_url: Some(resolved.artifact.url.clone()),
        artifact_sha256: Some(resolved.artifact.sha256.clone()),
        cache_path: Some(cache_path.display().to_string()),
        exposed_bins: exposed_bins.clone(),
        exposed_completions: exposed_completions.clone(),
        snapshot_id: options.snapshot_id.map(ToOwned::to_owned),
        install_mode: selected_install_mode,
        install_reason: determine_install_reason(
            &resolved.manifest.name,
            root_names,
            &receipts,
            &replacement_receipts,
        ),
        install_status: "installed".to_string(),
        installed_at_unix: current_unix_timestamp()?,
    };
    let receipt_path = write_install_receipt(layout, &receipt)?;

    Ok(InstallOutcome {
        name: resolved.manifest.name.clone(),
        version: resolved.manifest.version.to_string(),
        resolved_target: resolved.resolved_target.clone(),
        archive_type: resolved.archive_type,
        artifact_url: resolved.artifact.url.clone(),
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
        native_gui_records: native_gui_records
            .iter()
            .map(|record| record.key.clone())
            .collect(),
        warnings: native_gui_warnings,
    })
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
    let mut lines = vec![
        render_status_line(
            style,
            "ok",
            &format!(
                "resolved {} {} for {}",
                outcome.name, outcome.version, outcome.resolved_target
            ),
        ),
        render_status_line(
            style,
            "step",
            &format!("archive: {}", outcome.archive_type.as_str()),
        ),
        render_status_line(
            style,
            "step",
            &format!("artifact: {}", outcome.artifact_url),
        ),
        render_status_line(
            style,
            "step",
            &format!(
                "cache: {} ({})",
                outcome.cache_path.display(),
                outcome.download_status
            ),
        ),
        render_status_line(
            style,
            "step",
            &format!("install_root: {}", outcome.install_root.display()),
        ),
    ];

    if !outcome.exposed_bins.is_empty() {
        lines.push(render_status_line(
            style,
            "step",
            &format!("exposed_bins: {}", outcome.exposed_bins.join(", ")),
        ));
    }
    if !outcome.exposed_completions.is_empty() {
        lines.push(render_status_line(
            style,
            "step",
            &format!(
                "exposed_completions: {}",
                outcome.exposed_completions.join(", ")
            ),
        ));
    }
    if !outcome.exposed_gui_assets.is_empty() {
        lines.push(render_status_line(
            style,
            "step",
            &format!(
                "exposed_gui_assets: {}",
                outcome.exposed_gui_assets.join(", ")
            ),
        ));
    }
    if !outcome.native_gui_records.is_empty() {
        lines.push(render_status_line(
            style,
            "step",
            &format!(
                "native_gui_records: {}",
                outcome.native_gui_records.join(", ")
            ),
        ));
    }
    for warning in &outcome.warnings {
        lines.push(render_status_line(
            style,
            "warn",
            &format!("warning: {warning}"),
        ));
    }
    lines.push(render_status_line(
        style,
        "step",
        &format!("receipt: {}", outcome.receipt_path.display()),
    ));

    lines
}

fn print_install_outcome(outcome: &InstallOutcome, style: OutputStyle) {
    let renderer = TerminalRenderer::from_style(style);
    renderer.print_section(&format!("Installed {} {}", outcome.name, outcome.version));
    renderer.print_lines(&format_install_outcome_lines(outcome, style));
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

fn download_artifact(url: &str, cache_path: &Path, force_redownload: bool) -> Result<&'static str> {
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

    let result = if cfg!(windows) {
        download_with_powershell(url, &part_path)
    } else {
        download_with_curl(url, &part_path).or_else(|_| download_with_wget(url, &part_path))
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

fn download_with_curl(url: &str, out_path: &Path) -> Result<()> {
    let mut command = Command::new("curl");
    command
        .arg("-fL")
        .arg("--retry")
        .arg("2")
        .arg("-o")
        .arg(out_path)
        .arg(url);
    run_command(&mut command, "curl download failed")
}

fn download_with_wget(url: &str, out_path: &Path) -> Result<()> {
    let mut command = Command::new("wget");
    command.arg("-O").arg(out_path).arg(url);
    run_command(&mut command, "wget download failed")
}

fn download_with_powershell(url: &str, out_path: &Path) -> Result<()> {
    let mut command = Command::new("powershell");
    command.arg("-NoProfile").arg("-Command").arg(format!(
        "Invoke-WebRequest -Uri '{}' -OutFile '{}'",
        escape_ps_single_quote(url),
        escape_ps_single_quote_path(out_path)
    ));
    run_command(&mut command, "powershell download failed")
}

fn run_command(command: &mut Command, context_message: &str) -> Result<()> {
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

fn escape_ps_single_quote_path(path: &Path) -> String {
    let mut os = OsString::new();
    os.push(path.as_os_str());
    os.to_string_lossy().replace('\'', "''")
}
