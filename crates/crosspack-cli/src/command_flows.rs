fn ensure_upgrade_command_ready(layout: &PrefixLayout) -> Result<()> {
    layout.ensure_base_dirs()?;
    ensure_no_active_transaction_for(layout, "upgrade")
}

fn run_upgrade_command(
    layout: &PrefixLayout,
    registry_root: Option<&Path>,
    spec: Option<String>,
    dry_run: bool,
    provider_overrides: &BTreeMap<String, String>,
    interaction_policy: InstallInteractionPolicy,
) -> Result<()> {
    let output_style = current_output_style();
    ensure_upgrade_command_ready(layout)?;
    let backend = select_metadata_backend(registry_root, layout)?;

    let receipts = read_install_receipts(layout)?;
    if receipts.is_empty() {
        println!("No installed packages");
        return Ok(());
    }

    let snapshot_id = match registry_root {
        Some(_) => None,
        None => Some(resolve_transaction_snapshot_id(layout, "upgrade")?),
    };

    if dry_run {
        let mut planned_changes = Vec::new();

        match spec.as_deref() {
            Some(single) => {
                let (name, requirement) = parse_spec(single)?;
                let installed = receipts.iter().find(|receipt| receipt.name == name);
                let Some(installed_receipt) = installed else {
                    println!("{name} is not installed");
                    return Ok(());
                };

                let roots = vec![RootInstallRequest {
                    name: installed_receipt.name.clone(),
                    requirement,
                }];
                let resolved = resolve_install_graph(
                    layout,
                    &backend,
                    &roots,
                    installed_receipt.target.as_deref(),
                    provider_overrides,
                )?;
                enforce_no_downgrades(&receipts, &resolved, "upgrade")?;
                for package in &resolved {
                    validate_install_preflight_for_resolved(layout, package, &receipts)?;
                }
                planned_changes.extend(build_planned_package_changes(&resolved, &receipts)?);
            }
            None => {
                let plans = build_upgrade_plans(&receipts);
                if plans.is_empty() {
                    println!("{NO_ROOT_PACKAGES_TO_UPGRADE}");
                    return Ok(());
                }

                let mut grouped_resolved = Vec::new();
                let mut resolved_dependency_tokens = HashSet::new();
                for plan in &plans {
                    let (resolved, plan_tokens) = resolve_install_graph_with_tokens(
                        layout,
                        &backend,
                        &plan.roots,
                        plan.target.as_deref(),
                        provider_overrides,
                        false,
                    )?;
                    enforce_no_downgrades(&receipts, &resolved, "upgrade")?;
                    resolved_dependency_tokens.extend(plan_tokens);
                    grouped_resolved.push(resolved);
                }

                validate_provider_overrides_used(provider_overrides, &resolved_dependency_tokens)?;

                let overlap_check = grouped_resolved
                    .iter()
                    .zip(plans.iter())
                    .map(|(resolved, plan)| {
                        (
                            plan.target.as_deref(),
                            resolved
                                .iter()
                                .map(|package| package.manifest.name.clone())
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>();
                enforce_disjoint_multi_target_upgrade(&overlap_check)?;

                for resolved in &grouped_resolved {
                    for package in resolved {
                        validate_install_preflight_for_resolved(layout, package, &receipts)?;
                    }
                    planned_changes.extend(build_planned_package_changes(resolved, &receipts)?);
                }
            }
        }

        let preview = build_transaction_preview("upgrade", &planned_changes);
        for line in render_transaction_preview_lines(&preview, TransactionPreviewMode::DryRun) {
            println!("{line}");
        }
        return Ok(());
    }

    execute_with_transaction(layout, "upgrade", snapshot_id.as_deref(), |tx| {
        let mut journal_seq = 1_u64;

        match spec.as_deref() {
            Some(single) => {
                let (name, requirement) = parse_spec(single)?;
                let installed = receipts.iter().find(|receipt| receipt.name == name);
                let Some(installed_receipt) = installed else {
                    println!("{name} is not installed");
                    return Ok(());
                };

                let roots = vec![RootInstallRequest {
                    name: installed_receipt.name.clone(),
                    requirement,
                }];
                let root_names = Vec::new();
                let resolved = resolve_install_graph(
                    layout,
                    &backend,
                    &roots,
                    installed_receipt.target.as_deref(),
                    provider_overrides,
                )?;
                let planned_dependency_overrides = build_planned_dependency_overrides(&resolved);
                enforce_no_downgrades(&receipts, &resolved, "upgrade")?;

                append_transaction_journal_entry(
                    layout,
                    &tx.txid,
                    &TransactionJournalEntry {
                        seq: journal_seq,
                        step: format!("resolve_plan:{}", installed_receipt.name),
                        state: "done".to_string(),
                        path: Some(installed_receipt.name.clone()),
                    },
                )?;
                journal_seq += 1;

                for package in &resolved {
                    if let Some(old) = receipts.iter().find(|r| r.name == package.manifest.name) {
                        let old_version = Version::parse(&old.version).with_context(|| {
                            format!(
                                "installed receipt for '{}' has invalid version: {}",
                                old.name, old.version
                            )
                        })?;
                        if package.manifest.version <= old_version {
                            println!(
                                "{}",
                                render_status_line(
                                    output_style,
                                    "step",
                                    &format!(
                                        "{} is up-to-date ({})",
                                        package.manifest.name, old.version
                                    )
                                )
                            );
                            continue;
                        }
                    }

                    let snapshot_path =
                        capture_package_state_snapshot(layout, &tx.txid, &package.manifest.name)?;
                    append_transaction_journal_entry(
                        layout,
                        &tx.txid,
                        &TransactionJournalEntry {
                            seq: journal_seq,
                            step: format!("backup_package_state:{}", package.manifest.name),
                            state: "done".to_string(),
                            path: Some(snapshot_path.display().to_string()),
                        },
                    )?;
                    journal_seq += 1;

                    append_transaction_journal_entry(
                        layout,
                        &tx.txid,
                        &TransactionJournalEntry {
                            seq: journal_seq,
                            step: package_apply_step_name(
                                "upgrade",
                                &package.manifest.name,
                                install_mode_for_archive_type(package.archive_type),
                            ),
                            state: "done".to_string(),
                            path: Some(package.manifest.name.clone()),
                        },
                    )?;
                    journal_seq += 1;

                    let dependencies = build_dependency_receipts(package, &resolved);
                    let outcome = install_resolved(
                        layout,
                        package,
                        &dependencies,
                        &root_names,
                        &planned_dependency_overrides,
                        InstallResolvedOptions {
                            snapshot_id: snapshot_id.as_deref(),
                            force_redownload: false,
                            interaction_policy,
                        },
                    )?;
                    if let Some(old) = receipts.iter().find(|r| r.name == package.manifest.name) {
                        println!(
                            "{}",
                            render_status_line(
                                output_style,
                                "ok",
                                &format!(
                                    "upgraded {} from {} to {}",
                                    package.manifest.name, old.version, package.manifest.version
                                )
                            )
                        );
                    }
                    println!(
                        "{}",
                        render_status_line(
                            output_style,
                            "step",
                            &format!("receipt: {}", outcome.receipt_path.display())
                        )
                    );
                }
            }
            None => {
                let plans = build_upgrade_plans(&receipts);
                if plans.is_empty() {
                    println!("{NO_ROOT_PACKAGES_TO_UPGRADE}");
                    return Ok(());
                }

                let mut grouped_resolved = Vec::new();
                let mut resolved_dependency_tokens = HashSet::new();
                for plan in &plans {
                    let (resolved, plan_tokens) = resolve_install_graph_with_tokens(
                        layout,
                        &backend,
                        &plan.roots,
                        plan.target.as_deref(),
                        provider_overrides,
                        false,
                    )?;
                    enforce_no_downgrades(&receipts, &resolved, "upgrade")?;

                    append_transaction_journal_entry(
                        layout,
                        &tx.txid,
                        &TransactionJournalEntry {
                            seq: journal_seq,
                            step: format!(
                                "resolve_plan:{}",
                                plan.target.as_deref().unwrap_or("host")
                            ),
                            state: "done".to_string(),
                            path: plan.target.clone(),
                        },
                    )?;
                    journal_seq += 1;

                    resolved_dependency_tokens.extend(plan_tokens);
                    grouped_resolved.push(resolved);
                }

                validate_provider_overrides_used(provider_overrides, &resolved_dependency_tokens)?;

                let overlap_check = grouped_resolved
                    .iter()
                    .zip(plans.iter())
                    .map(|(resolved, plan)| {
                        (
                            plan.target.as_deref(),
                            resolved
                                .iter()
                                .map(|package| package.manifest.name.clone())
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>();
                enforce_disjoint_multi_target_upgrade(&overlap_check)?;

                for (resolved, plan) in grouped_resolved.iter().zip(plans.iter()) {
                    let planned_dependency_overrides = build_planned_dependency_overrides(resolved);

                    for package in resolved {
                        if let Some(old) = receipts.iter().find(|r| r.name == package.manifest.name)
                        {
                            let old_version = Version::parse(&old.version).with_context(|| {
                                format!(
                                    "installed receipt for '{}' has invalid version: {}",
                                    old.name, old.version
                                )
                            })?;
                            if package.manifest.version <= old_version {
                                println!(
                                    "{}",
                                    render_status_line(
                                        output_style,
                                        "step",
                                        &format!(
                                            "{} is up-to-date ({})",
                                            package.manifest.name, old.version
                                        )
                                    )
                                );
                                continue;
                            }
                        }

                        let snapshot_path = capture_package_state_snapshot(
                            layout,
                            &tx.txid,
                            &package.manifest.name,
                        )?;
                        append_transaction_journal_entry(
                            layout,
                            &tx.txid,
                            &TransactionJournalEntry {
                                seq: journal_seq,
                                step: format!("backup_package_state:{}", package.manifest.name),
                                state: "done".to_string(),
                                path: Some(snapshot_path.display().to_string()),
                            },
                        )?;
                        journal_seq += 1;

                        append_transaction_journal_entry(
                            layout,
                            &tx.txid,
                            &TransactionJournalEntry {
                                seq: journal_seq,
                                step: package_apply_step_name(
                                    "upgrade",
                                    &package.manifest.name,
                                    install_mode_for_archive_type(package.archive_type),
                                ),
                                state: "done".to_string(),
                                path: Some(package.manifest.name.clone()),
                            },
                        )?;
                        journal_seq += 1;

                        let dependencies = build_dependency_receipts(package, resolved);
                        let outcome = install_resolved(
                            layout,
                            package,
                            &dependencies,
                            &plan.root_names,
                            &planned_dependency_overrides,
                            InstallResolvedOptions {
                                snapshot_id: snapshot_id.as_deref(),
                                force_redownload: false,
                                interaction_policy,
                            },
                        )?;
                        if let Some(old) = receipts.iter().find(|r| r.name == package.manifest.name)
                        {
                            println!(
                                "{}",
                                render_status_line(
                                    output_style,
                                    "ok",
                                    &format!(
                                        "upgraded {} from {} to {}",
                                        package.manifest.name,
                                        old.version,
                                        package.manifest.version
                                    )
                                )
                            );
                        } else {
                            println!(
                                "{}",
                                render_status_line(
                                    output_style,
                                    "ok",
                                    &format!(
                                        "installed dependency {} {}",
                                        package.manifest.name, package.manifest.version
                                    )
                                )
                            );
                        }
                        println!(
                            "{}",
                            render_status_line(
                                output_style,
                                "step",
                                &format!("receipt: {}", outcome.receipt_path.display())
                            )
                        );
                    }
                }
            }
        }

        append_transaction_journal_entry(
            layout,
            &tx.txid,
            &TransactionJournalEntry {
                seq: journal_seq,
                step: "apply_complete".to_string(),
                state: "done".to_string(),
                path: None,
            },
        )?;

        Ok(())
    })?;

    if let Err(err) = sync_completion_assets_best_effort(layout, "upgrade") {
        eprintln!("{err}");
    }

    Ok(())
}

fn is_valid_txid_input(txid: &str) -> bool {
    !txid.is_empty()
        && txid.starts_with("tx-")
        && txid.len() <= 128
        && txid
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn txid_process_id(txid: &str) -> Option<u32> {
    txid.rsplit('-').next()?.parse().ok()
}

fn transaction_owner_process_alive(txid: &str) -> Result<bool> {
    let Some(pid) = txid_process_id(txid) else {
        return Ok(false);
    };

    #[cfg(unix)]
    {
        let status = Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| format!("failed executing owner liveness probe for pid={pid}"))?;
        Ok(status.success())
    }

    #[cfg(windows)]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .with_context(|| format!("failed executing owner liveness probe for pid={pid}"))?;

        if !output.status.success() {
            return Err(anyhow!(
                "owner liveness probe failed for pid={pid}: status={} stderr='{}'",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(stdout.contains(&format!(",\"{pid}\""))
            && !stdout.to_ascii_lowercase().contains("no tasks are running"))
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        Ok(true)
    }
}

fn read_transaction_journal_records(
    layout: &PrefixLayout,
    txid: &str,
) -> Result<Vec<TransactionJournalRecord>> {
    let path = layout.transaction_journal_path(txid);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("failed reading transaction journal: {}", path.display())
            });
        }
    };

    let mut records = Vec::new();
    for (line_no, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let value: Value = serde_json::from_str(trimmed).with_context(|| {
            format!(
                "failed parsing transaction journal entry: {} line={}",
                path.display(),
                line_no + 1
            )
        })?;
        let Some(object) = value.as_object() else {
            return Err(anyhow!(
                "failed parsing transaction journal entry: {} line={} is not an object",
                path.display(),
                line_no + 1
            ));
        };

        let seq = object
            .get("seq")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("missing journal field 'seq' line={}", line_no + 1))?;
        let step = object
            .get("step")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("missing journal field 'step' line={}", line_no + 1))?
            .to_string();
        let state = object
            .get("state")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("missing journal field 'state' line={}", line_no + 1))?
            .to_string();
        let path_value = object
            .get("path")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);

        records.push(TransactionJournalRecord {
            seq,
            step,
            state,
            path: path_value,
        });
    }

    records.sort_by_key(|record| record.seq);
    Ok(records)
}

fn rollback_package_from_step(step: &str) -> Option<&str> {
    step.strip_prefix("install_package:")
        .or_else(|| step.strip_prefix("install_native_package:"))
        .or_else(|| step.strip_prefix("upgrade_package:"))
        .or_else(|| step.strip_prefix("upgrade_native_package:"))
        .or_else(|| step.strip_prefix("uninstall_target:"))
        .or_else(|| step.strip_prefix("prune_dependency:"))
}

fn backup_package_from_step(step: &str) -> Option<&str> {
    step.strip_prefix("backup_package_state:")
}

fn package_apply_step_name(
    operation: &str,
    package_name: &str,
    install_mode: InstallMode,
) -> String {
    match install_mode {
        InstallMode::Managed => format!("{operation}_package:{package_name}"),
        InstallMode::Native => format!("{operation}_native_package:{package_name}"),
    }
}

fn snapshot_manifest_path(snapshot_root: &Path) -> PathBuf {
    snapshot_root.join("manifest.txt")
}

fn snapshot_package_root(snapshot_root: &Path) -> PathBuf {
    snapshot_root.join("package")
}

fn snapshot_receipt_path(snapshot_root: &Path, package_name: &str) -> PathBuf {
    snapshot_root
        .join("receipt")
        .join(format!("{package_name}.receipt"))
}

fn snapshot_bin_path(snapshot_root: &Path, bin_name: &str) -> PathBuf {
    snapshot_root.join("bins").join(bin_name)
}

fn snapshot_completions_root(snapshot_root: &Path) -> PathBuf {
    snapshot_root.join("completions")
}

fn snapshot_completion_path(snapshot_root: &Path, completion_storage_rel_path: &str) -> PathBuf {
    snapshot_completions_root(snapshot_root).join(completion_storage_rel_path)
}

fn snapshot_gui_root(snapshot_root: &Path) -> PathBuf {
    snapshot_root.join("gui")
}

fn snapshot_gui_asset_path(snapshot_root: &Path, gui_storage_rel_path: &str) -> PathBuf {
    snapshot_gui_root(snapshot_root).join(gui_storage_rel_path)
}

fn snapshot_native_sidecar_path(snapshot_root: &Path) -> PathBuf {
    snapshot_root.join("native").join("sidecar.state")
}

fn read_snapshot_manifest(snapshot_root: &Path) -> Result<PackageSnapshotManifest> {
    let path = snapshot_manifest_path(snapshot_root);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PackageSnapshotManifest {
                package_exists: false,
                receipt_exists: false,
                bins: Vec::new(),
                completions: Vec::new(),
                gui_assets: Vec::new(),
                native_sidecar_exists: false,
            });
        }
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed reading snapshot manifest: {}", path.display()));
        }
    };

    let mut manifest = PackageSnapshotManifest {
        package_exists: false,
        receipt_exists: false,
        bins: Vec::new(),
        completions: Vec::new(),
        gui_assets: Vec::new(),
        native_sidecar_exists: false,
    };

    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(value) = line.strip_prefix("package_exists=") {
            manifest.package_exists = value == "1";
        } else if let Some(value) = line.strip_prefix("receipt_exists=") {
            manifest.receipt_exists = value == "1";
        } else if let Some(bin_name) = line.strip_prefix("bin=") {
            manifest.bins.push(bin_name.to_string());
        } else if let Some(completion) = line.strip_prefix("completion=") {
            manifest.completions.push(completion.to_string());
        } else if let Some(gui_asset) = line.strip_prefix("gui_asset=") {
            let Some((key, rel_path)) = gui_asset.split_once('\t') else {
                return Err(anyhow!("invalid snapshot manifest gui_asset row"));
            };
            manifest.gui_assets.push(GuiExposureAsset {
                key: key.to_string(),
                rel_path: rel_path.to_string(),
            });
        } else if let Some(value) = line.strip_prefix("native_sidecar_exists=") {
            manifest.native_sidecar_exists = value == "1";
        }
    }

    Ok(manifest)
}

fn write_snapshot_manifest(snapshot_root: &Path, manifest: &PackageSnapshotManifest) -> Result<()> {
    let path = snapshot_manifest_path(snapshot_root);
    let mut lines = Vec::new();
    lines.push(format!(
        "package_exists={}",
        if manifest.package_exists { "1" } else { "0" }
    ));
    lines.push(format!(
        "receipt_exists={}",
        if manifest.receipt_exists { "1" } else { "0" }
    ));
    for bin in &manifest.bins {
        lines.push(format!("bin={bin}"));
    }
    for completion in &manifest.completions {
        lines.push(format!("completion={completion}"));
    }
    for asset in &manifest.gui_assets {
        lines.push(format!("gui_asset={}\t{}", asset.key, asset.rel_path));
    }
    lines.push(format!(
        "native_sidecar_exists={}",
        if manifest.native_sidecar_exists {
            "1"
        } else {
            "0"
        }
    ));
    std::fs::write(&path, lines.join("\n"))
        .with_context(|| format!("failed writing snapshot manifest: {}", path.display()))
}

fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(src)
        .with_context(|| format!("failed to stat source path: {}", src.display()))?;

    if metadata.is_dir() {
        std::fs::create_dir_all(dst)
            .with_context(|| format!("failed to create directory: {}", dst.display()))?;
        for entry in std::fs::read_dir(src)
            .with_context(|| format!("failed to read directory: {}", src.display()))?
        {
            let entry =
                entry.with_context(|| format!("failed to iterate directory: {}", src.display()))?;
            let child_src = entry.path();
            let child_dst = dst.join(entry.file_name());
            copy_tree(&child_src, &child_dst)?;
        }
        return Ok(());
    }

    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    }

    #[cfg(unix)]
    if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(src)
            .with_context(|| format!("failed to read symlink: {}", src.display()))?;
        std::os::unix::fs::symlink(&target, dst).with_context(|| {
            format!(
                "failed to copy symlink {} -> {}",
                dst.display(),
                target.display()
            )
        })?;
        return Ok(());
    }

    std::fs::copy(src, dst)
        .with_context(|| format!("failed to copy {} to {}", src.display(), dst.display()))?;
    Ok(())
}

fn capture_package_state_snapshot(
    layout: &PrefixLayout,
    txid: &str,
    package_name: &str,
) -> Result<PathBuf> {
    let snapshot_root = layout
        .transaction_staging_path(txid)
        .join("rollback")
        .join(package_name);
    if snapshot_root.exists() {
        std::fs::remove_dir_all(&snapshot_root).with_context(|| {
            format!(
                "failed clearing existing rollback snapshot dir: {}",
                snapshot_root.display()
            )
        })?;
    }

    std::fs::create_dir_all(snapshot_package_root(&snapshot_root)).with_context(|| {
        format!(
            "failed creating rollback snapshot package dir: {}",
            snapshot_package_root(&snapshot_root).display()
        )
    })?;
    std::fs::create_dir_all(snapshot_root.join("receipt")).with_context(|| {
        format!(
            "failed creating rollback snapshot receipt dir: {}",
            snapshot_root.join("receipt").display()
        )
    })?;
    std::fs::create_dir_all(snapshot_root.join("bins")).with_context(|| {
        format!(
            "failed creating rollback snapshot bins dir: {}",
            snapshot_root.join("bins").display()
        )
    })?;
    std::fs::create_dir_all(snapshot_completions_root(&snapshot_root)).with_context(|| {
        format!(
            "failed creating rollback snapshot completions dir: {}",
            snapshot_completions_root(&snapshot_root).display()
        )
    })?;
    std::fs::create_dir_all(snapshot_gui_root(&snapshot_root)).with_context(|| {
        format!(
            "failed creating rollback snapshot gui dir: {}",
            snapshot_gui_root(&snapshot_root).display()
        )
    })?;
    let snapshot_native_dir = snapshot_native_sidecar_path(&snapshot_root)
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("failed resolving rollback snapshot native state directory"))?;
    std::fs::create_dir_all(&snapshot_native_dir).with_context(|| {
        format!(
            "failed creating rollback snapshot native state dir: {}",
            snapshot_native_dir.display()
        )
    })?;

    let mut manifest = PackageSnapshotManifest {
        package_exists: false,
        receipt_exists: false,
        bins: Vec::new(),
        completions: Vec::new(),
        gui_assets: Vec::new(),
        native_sidecar_exists: false,
    };

    let package_root = layout.pkgs_dir().join(package_name);
    if package_root.exists() {
        manifest.package_exists = true;
        copy_tree(&package_root, &snapshot_package_root(&snapshot_root))?;
    }

    let receipt_path = layout.receipt_path(package_name);
    if receipt_path.exists() {
        manifest.receipt_exists = true;
        std::fs::copy(
            &receipt_path,
            snapshot_receipt_path(&snapshot_root, package_name),
        )
        .with_context(|| {
            format!(
                "failed copying receipt snapshot {}",
                snapshot_receipt_path(&snapshot_root, package_name).display()
            )
        })?;

        if let Some(receipt) = read_install_receipts(layout)?
            .into_iter()
            .find(|receipt| receipt.name == package_name)
        {
            manifest.bins = receipt.exposed_bins.clone();
            for bin_name in &manifest.bins {
                let source = bin_path(layout, bin_name);
                if source.exists() {
                    std::fs::copy(&source, snapshot_bin_path(&snapshot_root, bin_name))
                        .with_context(|| {
                            format!(
                                "failed copying binary snapshot {}",
                                snapshot_bin_path(&snapshot_root, bin_name).display()
                            )
                        })?;
                }
            }

            manifest.completions = receipt.exposed_completions.clone();
            for completion in &manifest.completions {
                let source = exposed_completion_path(layout, completion)?;
                if source.exists() {
                    copy_tree(
                        &source,
                        &snapshot_completion_path(&snapshot_root, completion),
                    )?;
                }
            }
        }
    }

    manifest.gui_assets = read_gui_exposure_state(layout, package_name)?;
    for gui_asset in &manifest.gui_assets {
        let source = gui_asset_path(layout, &gui_asset.rel_path)?;
        if source.exists() {
            copy_tree(
                &source,
                &snapshot_gui_asset_path(&snapshot_root, &gui_asset.rel_path),
            )?;
        }
    }

    let native_sidecar_path = layout.gui_native_state_path(package_name);
    if native_sidecar_path.exists() {
        manifest.native_sidecar_exists = true;
        std::fs::copy(
            &native_sidecar_path,
            snapshot_native_sidecar_path(&snapshot_root),
        )
        .with_context(|| {
            format!(
                "failed copying native sidecar snapshot {}",
                snapshot_native_sidecar_path(&snapshot_root).display()
            )
        })?;
    }

    write_snapshot_manifest(&snapshot_root, &manifest)?;
    Ok(snapshot_root)
}

fn binary_entry_points_to_package_root(bin_entry: &Path, package_root: &Path) -> Result<bool> {
    #[cfg(unix)]
    {
        let metadata = std::fs::symlink_metadata(bin_entry)
            .with_context(|| format!("failed to inspect binary entry: {}", bin_entry.display()))?;
        if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(bin_entry).with_context(|| {
                format!(
                    "failed to read binary symlink target: {}",
                    bin_entry.display()
                )
            })?;
            let resolved = if target.is_absolute() {
                target
            } else {
                bin_entry
                    .parent()
                    .map(|parent| parent.join(&target))
                    .unwrap_or(target)
            };
            return Ok(resolved.starts_with(package_root));
        }
        Ok(false)
    }

    #[cfg(windows)]
    {
        let metadata = std::fs::metadata(bin_entry)
            .with_context(|| format!("failed to inspect binary entry: {}", bin_entry.display()))?;
        if !metadata.is_file() {
            return Ok(false);
        }

        let shim = std::fs::read_to_string(bin_entry)
            .with_context(|| format!("failed to read binary shim: {}", bin_entry.display()))?;
        let Some(start) = shim.find('"') else {
            return Ok(false);
        };
        let rest = &shim[start + 1..];
        let Some(end) = rest.find('"') else {
            return Ok(false);
        };

        let source = PathBuf::from(&rest[..end]);
        Ok(source.starts_with(package_root))
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = bin_entry;
        let _ = package_root;
        Ok(false)
    }
}

fn remove_binary_entries_for_package_root(
    layout: &PrefixLayout,
    package_root: &Path,
) -> Result<()> {
    let entries = match std::fs::read_dir(layout.bin_dir()) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "failed to read bin directory: {}",
                    layout.bin_dir().display()
                )
            });
        }
    };

    for entry in entries {
        let entry = entry.with_context(|| {
            format!(
                "failed to iterate bin directory: {}",
                layout.bin_dir().display()
            )
        })?;
        let path = entry.path();
        if binary_entry_points_to_package_root(&path, package_root)? {
            remove_file_if_exists(&path)?;
        }
    }

    Ok(())
}

fn restore_package_state_snapshot(
    layout: &PrefixLayout,
    package_name: &str,
    snapshot_root: Option<&Path>,
) -> Result<()> {
    let package_root = layout.pkgs_dir().join(package_name);
    let existing_receipt = read_install_receipts(layout)?
        .into_iter()
        .find(|receipt| receipt.name == package_name);
    let native_records = read_gui_native_state(layout, package_name)?;
    let has_native_sidecar = !native_records.is_empty();
    let existing_receipt_mode = existing_receipt
        .as_ref()
        .map(|receipt| receipt.install_mode)
        .unwrap_or(InstallMode::Managed);
    let should_run_native_cleanup = existing_receipt_mode == InstallMode::Native
        || (existing_receipt.is_none() && has_native_sidecar);

    if should_run_native_cleanup {
        run_package_native_uninstall_actions(layout, package_name)?;
    }

    remove_binary_entries_for_package_root(layout, &package_root)?;

    let existing_bins = existing_receipt
        .as_ref()
        .map(|receipt| receipt.exposed_bins.clone())
        .unwrap_or_default();
    for bin_name in existing_bins {
        remove_exposed_binary(layout, &bin_name)?;
    }

    let existing_completions = existing_receipt
        .as_ref()
        .map(|receipt| receipt.exposed_completions.clone())
        .unwrap_or_default();
    for completion in existing_completions {
        remove_exposed_completion(layout, &completion)?;
    }

    let existing_gui_assets = read_gui_exposure_state(layout, package_name)?;
    for gui_asset in &existing_gui_assets {
        remove_exposed_gui_asset(layout, gui_asset)?;
    }
    write_gui_exposure_state(layout, package_name, &[])?;

    if !should_run_native_cleanup && !native_records.is_empty() {
        let _native_warnings = remove_native_gui_registration_best_effort(&native_records)?;
    }
    write_gui_native_state(layout, package_name, &[])?;

    if package_root.exists() {
        std::fs::remove_dir_all(&package_root).with_context(|| {
            format!("failed to remove package path: {}", package_root.display())
        })?;
    }

    remove_file_if_exists(&layout.receipt_path(package_name))?;

    let Some(snapshot_root) = snapshot_root else {
        return Ok(());
    };

    let PackageSnapshotManifest {
        package_exists,
        receipt_exists,
        bins,
        completions,
        gui_assets,
        native_sidecar_exists,
    } = read_snapshot_manifest(snapshot_root)?;

    if package_exists && snapshot_package_root(snapshot_root).exists() {
        copy_tree(&snapshot_package_root(snapshot_root), &package_root)?;
    }

    if receipt_exists {
        let src = snapshot_receipt_path(snapshot_root, package_name);
        if src.exists() {
            if let Some(parent) = layout.receipt_path(package_name).parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            std::fs::copy(&src, layout.receipt_path(package_name)).with_context(|| {
                format!(
                    "failed restoring receipt from {}",
                    snapshot_receipt_path(snapshot_root, package_name).display()
                )
            })?;
        }
    }

    for bin_name in bins {
        let dst = bin_path(layout, &bin_name);
        remove_file_if_exists(&dst)?;
        let src = snapshot_bin_path(snapshot_root, &bin_name);
        if src.exists() {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            std::fs::copy(&src, &dst).with_context(|| {
                format!(
                    "failed restoring binary '{}' from {}",
                    bin_name,
                    src.display()
                )
            })?;
        }
    }

    for completion in completions {
        let dst = exposed_completion_path(layout, &completion)?;
        remove_file_if_exists(&dst)?;
        let src = snapshot_completion_path(snapshot_root, &completion);
        if src.exists() {
            copy_tree(&src, &dst).with_context(|| {
                format!(
                    "failed restoring completion '{}' from {}",
                    completion,
                    src.display()
                )
            })?;
        }
    }

    for gui_asset in &gui_assets {
        let dst = gui_asset_path(layout, &gui_asset.rel_path)?;
        remove_file_if_exists(&dst)?;
        let src = snapshot_gui_asset_path(snapshot_root, &gui_asset.rel_path);
        if src.exists() {
            copy_tree(&src, &dst).with_context(|| {
                format!(
                    "failed restoring gui asset '{}' from {}",
                    gui_asset.key,
                    src.display()
                )
            })?;
        }
    }
    write_gui_exposure_state(layout, package_name, &gui_assets)?;

    if native_sidecar_exists {
        let dst = layout.gui_native_state_path(package_name);
        let src = snapshot_native_sidecar_path(snapshot_root);
        remove_file_if_exists(&dst)?;
        if src.exists() {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            std::fs::copy(&src, &dst).with_context(|| {
                format!(
                    "failed restoring native sidecar state from {}",
                    src.display()
                )
            })?;
        }
    }

    Ok(())
}

fn replay_rollback_journal(layout: &PrefixLayout, txid: &str) -> Result<bool> {
    let records = read_transaction_journal_records(layout, txid)?;
    if records.is_empty() {
        return Ok(false);
    }

    let mut backups = HashMap::new();
    for record in &records {
        if record.state != "done" {
            continue;
        }
        if let Some(package_name) = backup_package_from_step(&record.step) {
            if let Some(path) = &record.path {
                backups.insert(package_name.to_string(), PathBuf::from(path));
            }
        }
    }

    let mut compensating_steps = records
        .iter()
        .filter(|record| record.state == "done")
        .filter_map(|record| {
            rollback_package_from_step(&record.step)
                .map(|package_name| (record.seq, package_name.to_string()))
        })
        .collect::<Vec<_>>();
    compensating_steps.sort_by(|left, right| right.0.cmp(&left.0));

    if compensating_steps.is_empty() {
        return Ok(false);
    }

    for (_, package_name) in &compensating_steps {
        if !backups.contains_key(package_name) {
            return Err(anyhow!(
                "transaction journal missing rollback payload for package '{package_name}'"
            ));
        }
    }

    for (_, package_name) in compensating_steps {
        let snapshot_root = backups.get(&package_name).map(PathBuf::as_path);
        restore_package_state_snapshot(layout, &package_name, snapshot_root)?;
    }

    Ok(true)
}

fn latest_rollback_candidate_txid(layout: &PrefixLayout) -> Result<Option<String>> {
    let entries = match std::fs::read_dir(layout.transactions_dir()) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "failed to read transactions directory: {}",
                    layout.transactions_dir().display()
                )
            })
        }
    };

    let mut latest: Option<(u64, String)> = None;
    for entry in entries {
        let entry = entry.with_context(|| {
            format!(
                "failed to iterate transactions directory: {}",
                layout.transactions_dir().display()
            )
        })?;
        let path = entry.path();

        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        let Some(txid) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };

        let Some(metadata) = read_transaction_metadata(layout, txid)? else {
            continue;
        };
        if matches!(metadata.status.as_str(), "committed" | "rolled_back") {
            continue;
        }

        match &latest {
            None => latest = Some((metadata.started_at_unix, metadata.txid)),
            Some((best_started_at, best_txid)) => {
                if metadata.started_at_unix > *best_started_at
                    || (metadata.started_at_unix == *best_started_at && metadata.txid > *best_txid)
                {
                    latest = Some((metadata.started_at_unix, metadata.txid));
                }
            }
        }
    }

    Ok(latest.map(|(_, txid)| txid))
}

fn run_rollback_command(layout: &PrefixLayout, txid: Option<String>) -> Result<()> {
    let output_style = current_output_style();
    layout.ensure_base_dirs()?;

    let target_txid = match txid {
        Some(txid) => {
            if !is_valid_txid_input(&txid) {
                return Err(anyhow!("invalid rollback txid: {txid}"));
            }
            txid
        }
        None => {
            if let Some(active_txid) = read_active_transaction(layout)? {
                active_txid
            } else if let Some(candidate_txid) = latest_rollback_candidate_txid(layout)? {
                candidate_txid
            } else {
                println!(
                    "{}",
                    render_status_line(output_style, "step", "no rollback needed")
                );
                return Ok(());
            }
        }
    };

    let metadata = read_transaction_metadata(layout, &target_txid)?
        .ok_or_else(|| anyhow!("transaction metadata missing for rollback txid={target_txid}"))?;
    let active_txid = read_active_transaction(layout)?;

    if matches!(metadata.status.as_str(), "planning" | "applying")
        && active_txid.as_deref() == Some(target_txid.as_str())
        && transaction_owner_process_alive(&target_txid)?
    {
        return Err(anyhow!(
            "cannot rollback while transaction is active (status={})",
            metadata.status
        ));
    }

    if metadata.status == "committed" || metadata.status == "rolled_back" {
        if active_txid.as_deref() == Some(target_txid.as_str()) {
            clear_active_transaction(layout)?;
        }
        println!(
            "{}",
            render_status_line(output_style, "step", "no rollback needed")
        );
        return Ok(());
    }

    let journal_records = read_transaction_journal_records(layout, &target_txid)?;
    let has_completed_mutating_steps = journal_records
        .iter()
        .any(|record| record.state == "done" && rollback_package_from_step(&record.step).is_some());

    set_transaction_status(layout, &target_txid, "rolling_back")?;
    let replayed = match replay_rollback_journal(layout, &target_txid) {
        Ok(replayed) => replayed,
        Err(err) => {
            let _ = set_transaction_status(layout, &target_txid, "failed");
            return Err(err).with_context(|| {
                format!("rollback failed {target_txid}: transaction journal replay required")
            });
        }
    };

    if !replayed && has_completed_mutating_steps {
        let _ = set_transaction_status(layout, &target_txid, "failed");
        return Err(anyhow!(
            "rollback failed {target_txid}: transaction journal replay required"
        ));
    }

    set_transaction_status(layout, &target_txid, "rolled_back")?;

    if active_txid.as_deref() == Some(target_txid.as_str()) {
        clear_active_transaction(layout)?;
    }

    if let Err(err) = sync_completion_assets_best_effort(layout, "rollback") {
        eprintln!("{err}");
    }

    println!(
        "{}",
        render_status_line(output_style, "ok", &format!("rolled back {target_txid}"))
    );
    Ok(())
}

fn run_repair_command(layout: &PrefixLayout) -> Result<()> {
    let output_style = current_output_style();
    layout.ensure_base_dirs()?;

    let Some(txid) = read_active_transaction(layout)? else {
        println!(
            "{}",
            render_status_line(output_style, "step", "repair: no action needed")
        );
        return Ok(());
    };

    let metadata = read_transaction_metadata(layout, &txid)?;
    let Some(metadata) = metadata else {
        clear_active_transaction(layout)?;
        println!(
            "{}",
            render_status_line(
                output_style,
                "ok",
                &format!("repair: cleared stale marker {txid}")
            )
        );
        return Ok(());
    };

    if status_allows_stale_marker_cleanup(&metadata.status) {
        clear_active_transaction(layout)?;
        println!(
            "{}",
            render_status_line(
                output_style,
                "ok",
                &format!("repair: cleared stale marker {txid}")
            )
        );
        return Ok(());
    }

    match metadata.status.as_str() {
        "planning" | "applying" | "failed" | "rolling_back" => {
            run_rollback_command(layout, Some(txid.clone()))?;
            println!(
                "{}",
                render_status_line(
                    output_style,
                    "ok",
                    &format!("recovered interrupted transaction {txid}: rolled back")
                )
            );
            Ok(())
        }
        status => Err(anyhow!(
            "transaction {txid} requires manual repair (reason=unsupported_status status={status})"
        )),
    }
}

fn run_uninstall_command(layout: &PrefixLayout, name: String) -> Result<()> {
    let output_style = current_output_style();
    layout.ensure_base_dirs()?;
    ensure_no_active_transaction_for(layout, "uninstall")?;

    execute_with_transaction(layout, "uninstall", None, |tx| {
        let mut journal_seq = 1_u64;
        let mut snapshot_paths = HashMap::new();
        for receipt in read_install_receipts(layout)? {
            let snapshot_path = capture_package_state_snapshot(layout, &tx.txid, &receipt.name)?;
            snapshot_paths.insert(receipt.name, snapshot_path);
        }

        let result = uninstall_package(layout, &name)?;

        if let Some(snapshot_path) = snapshot_paths.get(&name) {
            append_transaction_journal_entry(
                layout,
                &tx.txid,
                &TransactionJournalEntry {
                    seq: journal_seq,
                    step: format!("backup_package_state:{}", name),
                    state: "done".to_string(),
                    path: Some(snapshot_path.display().to_string()),
                },
            )?;
            journal_seq += 1;
        }

        append_transaction_journal_entry(
            layout,
            &tx.txid,
            &TransactionJournalEntry {
                seq: journal_seq,
                step: format!("uninstall_target:{}", name),
                state: "done".to_string(),
                path: Some(name.clone()),
            },
        )?;
        journal_seq += 1;

        for dependency in &result.pruned_dependencies {
            if let Some(snapshot_path) = snapshot_paths.get(dependency) {
                append_transaction_journal_entry(
                    layout,
                    &tx.txid,
                    &TransactionJournalEntry {
                        seq: journal_seq,
                        step: format!("backup_package_state:{dependency}"),
                        state: "done".to_string(),
                        path: Some(snapshot_path.display().to_string()),
                    },
                )?;
                journal_seq += 1;
            }

            append_transaction_journal_entry(
                layout,
                &tx.txid,
                &TransactionJournalEntry {
                    seq: journal_seq,
                    step: format!("prune_dependency:{dependency}"),
                    state: "done".to_string(),
                    path: Some(dependency.clone()),
                },
            )?;
            journal_seq += 1;
        }

        append_transaction_journal_entry(
            layout,
            &tx.txid,
            &TransactionJournalEntry {
                seq: journal_seq,
                step: "apply_complete".to_string(),
                state: "done".to_string(),
                path: None,
            },
        )?;

        let status = if matches!(result.status, UninstallStatus::BlockedByDependents) {
            "warn"
        } else {
            "ok"
        };
        for line in format_uninstall_messages(&result) {
            println!("{}", render_status_line(output_style, status, &line));
        }

        Ok(())
    })?;

    if let Err(err) = sync_completion_assets_best_effort(layout, "uninstall") {
        eprintln!("{err}");
    }

    Ok(())
}

fn run_update_command(store: &RegistrySourceStore, registry: &[String]) -> Result<()> {
    let output_style = current_output_style();
    let results = store.update_sources(registry)?;
    let report = build_update_report(&results);
    for line in format_update_output_lines(&report, output_style) {
        println!("{line}");
    }
    println!(
        "{}",
        format_update_summary_line(report.updated, report.up_to_date, report.failed)
    );
    ensure_update_succeeded(report.failed)
}

fn run_self_update_command(
    layout: &PrefixLayout,
    registry_root: Option<&Path>,
    dry_run: bool,
    force_redownload: bool,
    escalation: EscalationArgs,
) -> Result<()> {
    let _escalation_policy = resolve_escalation_policy(escalation);
    let output_style = current_output_style();
    layout.ensure_base_dirs()?;
    ensure_no_active_transaction_for(layout, "self-update")?;

    if registry_root.is_none() {
        println!(
            "{}",
            render_status_line(
                output_style,
                "step",
                "self-update: refreshing source snapshots"
            )
        );
        let source_state_root = registry_state_root(layout);
        let store = RegistrySourceStore::new(&source_state_root);
        run_update_command(&store, &[])?;
    }

    let args = build_self_update_install_args(registry_root, dry_run, force_redownload, escalation);
    println!(
        "{}",
        render_status_line(
            output_style,
            "step",
            "self-update: installing latest crosspack"
        )
    );
    run_current_exe_command(&args, "self-update install")
}

fn build_self_update_install_args(
    registry_root: Option<&Path>,
    dry_run: bool,
    force_redownload: bool,
    escalation: EscalationArgs,
) -> Vec<OsString> {
    let mut args = Vec::new();
    if let Some(root) = registry_root {
        args.push(OsString::from("--registry-root"));
        args.push(root.as_os_str().to_os_string());
    }

    args.push(OsString::from("install"));
    args.push(OsString::from("crosspack"));

    if dry_run {
        args.push(OsString::from("--dry-run"));
    }
    if force_redownload {
        args.push(OsString::from("--force-redownload"));
    }
    if escalation.non_interactive {
        args.push(OsString::from("--non-interactive"));
    }
    if escalation.allow_escalation {
        args.push(OsString::from("--allow-escalation"));
    }
    if escalation.no_escalation {
        args.push(OsString::from("--no-escalation"));
    }

    args
}

fn run_current_exe_command(args: &[OsString], context: &str) -> Result<()> {
    let current_exe = std::env::current_exe().context("failed to resolve current executable")?;
    let status = Command::new(&current_exe)
        .args(args)
        .status()
        .with_context(|| format!("failed to launch {} via {}", context, current_exe.display()))?;
    if status.success() {
        return Ok(());
    }

    Err(anyhow!("{} failed with status {}", context, status))
}

fn format_registry_kind(kind: RegistrySourceKind) -> &'static str {
    match kind {
        RegistrySourceKind::Git => "git",
        RegistrySourceKind::Filesystem => "filesystem",
    }
}

fn format_registry_add_lines(
    name: &str,
    kind: &str,
    priority: u32,
    fingerprint: &str,
) -> Vec<String> {
    let prefix: String = fingerprint.chars().take(16).collect();
    vec![
        format!("added registry {name}"),
        format!("kind: {kind}"),
        format!("priority: {priority}"),
        format!("fingerprint: {prefix}..."),
    ]
}

fn format_registry_remove_lines(name: &str, purge_cache: bool) -> Vec<String> {
    let cache_state = if purge_cache { "purged" } else { "kept" };
    vec![
        format!("removed registry {name}"),
        format!("cache: {cache_state}"),
    ]
}

fn format_registry_list_snapshot_state(snapshot: &RegistrySourceSnapshotState) -> String {
    match snapshot {
        RegistrySourceSnapshotState::None => "none".to_string(),
        RegistrySourceSnapshotState::Ready { snapshot_id } => format!("ready:{snapshot_id}"),
        RegistrySourceSnapshotState::Error { reason_code, .. } => format!("error:{reason_code}"),
    }
}

fn format_registry_list_lines(mut sources: Vec<RegistrySourceWithSnapshotState>) -> Vec<String> {
    sources.sort_by(|left, right| {
        left.source
            .priority
            .cmp(&right.source.priority)
            .then_with(|| left.source.name.cmp(&right.source.name))
    });

    sources
        .into_iter()
        .map(|source| {
            let kind = format_registry_kind(source.source.kind.clone());
            format!(
                "{} kind={} priority={} location={} snapshot={}",
                source.source.name,
                kind,
                source.source.priority,
                source.source.location,
                format_registry_list_snapshot_state(&source.snapshot)
            )
        })
        .collect()
}
