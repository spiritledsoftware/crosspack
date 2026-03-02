const BUNDLE_FORMAT_MARKER: &str = "crosspack.bundle";
const BUNDLE_FORMAT_VERSION: u32 = 1;
const DEFAULT_BUNDLE_FILE: &str = "crosspack.bundle.toml";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BundleDocument {
    format: String,
    version: u32,
    #[serde(default)]
    roots: Vec<BundleRoot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot_context: Option<BundleSnapshotContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BundleRoot {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requirement: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BundleSnapshotContext {
    sources: Vec<BundleSnapshotSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BundleSnapshotSource {
    name: String,
    enabled: bool,
    snapshot: String,
}

#[derive(Debug, Clone)]
struct BundleApplyGroupPlan {
    target: Option<String>,
    roots: Vec<RootInstallRequest>,
    root_names: Vec<String>,
    resolved: Vec<ResolvedInstall>,
}

#[derive(Debug, Clone)]
struct BundleApplyOptions<'a> {
    file: Option<&'a Path>,
    dry_run: bool,
    explain: bool,
    build_from_source: bool,
    force_redownload: bool,
    provider_values: &'a [String],
}

fn default_bundle_file_path() -> PathBuf {
    PathBuf::from(DEFAULT_BUNDLE_FILE)
}

fn run_bundle_command(
    layout: &PrefixLayout,
    registry_root: Option<&Path>,
    command: BundleCommands,
) -> Result<()> {
    match command {
        BundleCommands::Export { output } => run_bundle_export_command(layout, output.as_deref()),
        BundleCommands::Apply {
            file,
            dry_run,
            explain,
            build_from_source,
            force_redownload,
            provider,
        } => run_bundle_apply_command(
            layout,
            registry_root,
            BundleApplyOptions {
                file: file.as_deref(),
                dry_run,
                explain,
                build_from_source,
                force_redownload,
                provider_values: &provider,
            },
        ),
    }
}

fn run_bundle_export_command(layout: &PrefixLayout, output: Option<&Path>) -> Result<()> {
    layout.ensure_base_dirs()?;
    let bundle = build_export_bundle_document(layout)?;
    let rendered = render_bundle_document(&bundle)?;

    match output {
        Some(path) => {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "failed creating bundle output directory: {}",
                        parent.display()
                    )
                })?;
            }
            fs::write(path, rendered)
                .with_context(|| format!("failed writing bundle file: {}", path.display()))?;
            println!("bundle exported: {}", path.display());
        }
        None => {
            print!("{rendered}");
        }
    }

    Ok(())
}

fn run_bundle_apply_command(
    layout: &PrefixLayout,
    registry_root: Option<&Path>,
    options: BundleApplyOptions<'_>,
) -> Result<()> {
    ensure_explain_requires_dry_run("bundle apply", options.dry_run, options.explain)?;
    layout.ensure_base_dirs()?;
    ensure_no_active_transaction_for(layout, "bundle apply")?;
    let provider_overrides = parse_provider_overrides(options.provider_values)?;

    let bundle_path = options
        .file
        .map(Path::to_path_buf)
        .unwrap_or_else(default_bundle_file_path);
    let bundle = load_bundle_document_from_path(&bundle_path)?;

    let backend = select_metadata_backend(registry_root, layout)?;
    let group_plans = build_bundle_apply_group_plans(
        layout,
        &backend,
        &bundle,
        &provider_overrides,
        options.build_from_source,
    )?;
    let receipts = read_install_receipts(layout)?;
    for plan in &group_plans {
        for package in &plan.resolved {
            validate_install_preflight_for_resolved(layout, package, &receipts)?;
        }
    }

    let mut planned_changes = Vec::new();
    for plan in &group_plans {
        planned_changes.extend(build_planned_package_changes(&plan.resolved, &receipts)?);
    }

    if options.dry_run {
        let preview = build_transaction_preview("bundle-apply", &planned_changes);
        let mut explainability = DependencyPolicyExplainability::default();
        if options.explain {
            for plan in &group_plans {
                merge_dependency_policy_explainability(
                    &mut explainability,
                    build_dependency_policy_explainability(&plan.resolved, &receipts, &plan.roots)?,
                );
            }
        }
        for line in render_dry_run_output_lines(
            &preview,
            TransactionPreviewMode::DryRun,
            options.explain.then_some(&explainability),
        ) {
            println!("{line}");
        }
        return Ok(());
    }

    let output_style = current_output_style();
    let install_progress_mode = current_install_progress_mode(output_style);
    let snapshot_id = match registry_root {
        Some(_) => None,
        None => Some(resolve_transaction_snapshot_id(layout, "bundle-apply")?),
    };

    execute_with_transaction(layout, "bundle-apply", snapshot_id.as_deref(), |tx| {
        let interaction_policy = InstallInteractionPolicy::default();
        let mut journal_seq = 1_u64;
        for plan in &group_plans {
            append_transaction_journal_entry(
                layout,
                &tx.txid,
                &TransactionJournalEntry {
                    seq: journal_seq,
                    step: format!("resolve_plan:{}", plan.target.as_deref().unwrap_or("host")),
                    state: "done".to_string(),
                    path: plan.target.clone(),
                },
            )?;
            journal_seq += 1;

            let planned_dependency_overrides = build_planned_dependency_overrides(&plan.resolved);
            for package in &plan.resolved {
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
                            "install",
                            &package.manifest.name,
                            install_mode_for_archive_type(package.archive_type),
                        ),
                        state: "done".to_string(),
                        path: Some(package.manifest.name.clone()),
                    },
                )?;
                journal_seq += 1;

                let dependencies = build_dependency_receipts(package, &plan.resolved);
                let mut source_build_journal = SourceBuildJournal {
                    txid: &tx.txid,
                    seq: &mut journal_seq,
                };
                let outcome = install_resolved(
                    layout,
                    package,
                    &dependencies,
                    &plan.root_names,
                    &planned_dependency_overrides,
                    InstallResolvedOptions {
                        snapshot_id: snapshot_id.as_deref(),
                        force_redownload: options.force_redownload,
                        interaction_policy,
                        install_progress_mode,
                    },
                    Some(&mut source_build_journal),
                )?;
                print_install_outcome(&outcome, output_style);
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

    if let Err(err) = sync_completion_assets_best_effort(layout, "bundle-apply") {
        eprintln!("{err}");
    }

    Ok(())
}

fn build_export_bundle_document(layout: &PrefixLayout) -> Result<BundleDocument> {
    let receipts = read_install_receipts(layout)?;
    let pins = read_all_pins(layout)?;

    let mut roots = receipts
        .into_iter()
        .filter(|receipt| receipt.install_reason == InstallReason::Root)
        .map(|receipt| BundleRoot {
            requirement: pins
                .get(&receipt.name)
                .cloned()
                .or_else(|| Some(format!("={}", receipt.version))),
            name: receipt.name,
            target: receipt.target,
        })
        .collect::<Vec<_>>();
    roots.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.target.cmp(&right.target))
            .then_with(|| left.requirement.cmp(&right.requirement))
    });

    let snapshot_context = load_bundle_snapshot_context_best_effort(layout);

    Ok(BundleDocument {
        format: BUNDLE_FORMAT_MARKER.to_string(),
        version: BUNDLE_FORMAT_VERSION,
        roots,
        snapshot_context,
    })
}

fn render_bundle_document(bundle: &BundleDocument) -> Result<String> {
    let mut rendered = toml::to_string_pretty(bundle).context("failed rendering bundle as TOML")?;
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    Ok(rendered)
}

fn load_bundle_snapshot_context_best_effort(
    layout: &PrefixLayout,
) -> Option<BundleSnapshotContext> {
    let source_state_root = registry_state_root(layout);
    let store = RegistrySourceStore::new(&source_state_root);
    let mut sources = match store.list_sources_with_snapshot_state() {
        Ok(sources) => sources,
        Err(_) => return None,
    };
    if sources.is_empty() {
        return None;
    }

    sources.sort_by(|left, right| left.source.name.cmp(&right.source.name));
    let mapped = sources
        .into_iter()
        .map(|source| BundleSnapshotSource {
            name: source.source.name,
            enabled: source.source.enabled,
            snapshot: bundle_snapshot_token(&source.snapshot),
        })
        .collect::<Vec<_>>();
    Some(BundleSnapshotContext { sources: mapped })
}

fn bundle_snapshot_token(snapshot: &RegistrySourceSnapshotState) -> String {
    match snapshot {
        RegistrySourceSnapshotState::Ready { snapshot_id } => format!("ready:{snapshot_id}"),
        RegistrySourceSnapshotState::None => "none".to_string(),
        RegistrySourceSnapshotState::Error { reason_code, .. } => format!("error:{reason_code}"),
    }
}

fn parse_bundle_document(raw: &str) -> Result<BundleDocument> {
    let bundle: BundleDocument = toml::from_str(raw).context("failed parsing bundle TOML")?;
    validate_bundle_document(&bundle)?;
    Ok(bundle)
}

fn load_bundle_document_from_path(path: &Path) -> Result<BundleDocument> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow!(
                "bundle file not found: {} (use --file <path> or create {})",
                path.display(),
                DEFAULT_BUNDLE_FILE
            ));
        }
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed reading bundle file: {}", path.display()));
        }
    };
    parse_bundle_document(&raw).with_context(|| format!("invalid bundle file: {}", path.display()))
}

fn validate_bundle_document(bundle: &BundleDocument) -> Result<()> {
    if bundle.format != BUNDLE_FORMAT_MARKER {
        return Err(anyhow!(
            "unsupported bundle format marker '{}': expected '{}'",
            bundle.format,
            BUNDLE_FORMAT_MARKER
        ));
    }
    if bundle.version != BUNDLE_FORMAT_VERSION {
        return Err(anyhow!(
            "unsupported bundle format version '{}': expected {}",
            bundle.version,
            BUNDLE_FORMAT_VERSION
        ));
    }
    if bundle.roots.is_empty() {
        return Err(anyhow!("bundle must include at least one root package"));
    }

    let mut seen = BTreeSet::new();
    for root in &bundle.roots {
        if !is_policy_token(&root.name) {
            return Err(anyhow!(
                "invalid bundle root package '{}': expected package-name grammar",
                root.name
            ));
        }
        if let Some(requirement) = &root.requirement {
            VersionReq::parse(requirement).with_context(|| {
                format!(
                    "invalid bundle requirement for '{}' in bundle: {}",
                    root.name, requirement
                )
            })?;
        }

        if !seen.insert((root.name.clone(), root.target.clone())) {
            return Err(anyhow!(
                "duplicate bundle root entry for package '{}' target '{}'",
                root.name,
                root.target.as_deref().unwrap_or("host")
            ));
        }
    }

    Ok(())
}

fn build_bundle_apply_group_plans(
    layout: &PrefixLayout,
    backend: &MetadataBackend,
    bundle: &BundleDocument,
    provider_overrides: &BTreeMap<String, String>,
    build_from_source: bool,
) -> Result<Vec<BundleApplyGroupPlan>> {
    let mut grouped_roots = BTreeMap::<Option<String>, Vec<RootInstallRequest>>::new();
    for root in &bundle.roots {
        let requirement = root.requirement.as_deref().unwrap_or("*");
        grouped_roots
            .entry(root.target.clone())
            .or_default()
            .push(RootInstallRequest {
                name: root.name.clone(),
                requirement: VersionReq::parse(requirement).with_context(|| {
                    format!(
                        "invalid bundle requirement for '{}' in bundle: {}",
                        root.name, requirement
                    )
                })?,
            });
    }

    let mut plans = Vec::new();
    let mut resolved_dependency_tokens = HashSet::new();
    for (target, mut roots) in grouped_roots {
        roots.sort_by(|left, right| left.name.cmp(&right.name));
        let root_names = roots
            .iter()
            .map(|root| root.name.clone())
            .collect::<Vec<_>>();
        let (resolved, plan_tokens) = resolve_install_graph_with_tokens(
            layout,
            backend,
            &roots,
            target.as_deref(),
            provider_overrides,
            false,
            build_from_source,
        )?;
        resolved_dependency_tokens.extend(plan_tokens);
        plans.push(BundleApplyGroupPlan {
            target,
            roots,
            root_names,
            resolved,
        });
    }

    validate_provider_overrides_used(provider_overrides, &resolved_dependency_tokens)?;
    let overlap_check = plans
        .iter()
        .map(|plan| {
            (
                plan.target.as_deref(),
                plan.resolved
                    .iter()
                    .map(|package| package.manifest.name.clone())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    enforce_disjoint_multi_target_upgrade(&overlap_check)?;
    plans.sort_by(|left, right| left.target.cmp(&right.target));
    Ok(plans)
}
