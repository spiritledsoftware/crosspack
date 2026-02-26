fn run_cli(cli: Cli) -> Result<()> {

    match cli.command {
        Commands::Search { query } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            let backend = select_metadata_backend(cli.registry_root.as_deref(), &layout)?;
            let results = run_search_command(&backend, &query)?;
            for line in format_search_results(&results, &query) {
                println!("{line}");
            }
        }
        Commands::Info { name } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            let backend = select_metadata_backend(cli.registry_root.as_deref(), &layout)?;
            let versions = backend.package_versions(&name)?;

            if versions.is_empty() {
                println!("No package found: {name}");
            } else {
                for line in format_info_lines(&name, &versions) {
                    println!("{line}");
                }
            }
        }
        Commands::Install {
            spec,
            target,
            dry_run,
            force_redownload,
            provider,
            escalation,
        } => {
            let (name, requirement) = parse_spec(&spec)?;
            let provider_overrides = parse_provider_overrides(&provider)?;
            let escalation_policy = resolve_escalation_policy(escalation);
            let interaction_policy = install_interaction_policy(escalation_policy);
            let output_style = current_output_style();

            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            layout.ensure_base_dirs()?;
            ensure_no_active_transaction_for(&layout, "install")?;
            let backend = select_metadata_backend(cli.registry_root.as_deref(), &layout)?;

            let snapshot_id = match cli.registry_root.as_deref() {
                Some(_) => None,
                None => Some(resolve_transaction_snapshot_id(&layout, "install")?),
            };
            if dry_run {
                let roots = vec![RootInstallRequest { name, requirement }];
                let resolved = resolve_install_graph(
                    &layout,
                    &backend,
                    &roots,
                    target.as_deref(),
                    &provider_overrides,
                )?;
                let receipts = read_install_receipts(&layout)?;
                for package in &resolved {
                    validate_install_preflight_for_resolved(&layout, package, &receipts)?;
                }
                let planned_changes = build_planned_package_changes(&resolved, &receipts)?;
                let preview = build_transaction_preview("install", &planned_changes);
                for line in
                    render_transaction_preview_lines(&preview, TransactionPreviewMode::DryRun)
                {
                    println!("{line}");
                }
                return Ok(());
            }

            execute_with_transaction(&layout, "install", snapshot_id.as_deref(), |tx| {
                let mut journal_seq = 1_u64;
                let roots = vec![RootInstallRequest { name, requirement }];
                let root_names = roots
                    .iter()
                    .map(|root| root.name.clone())
                    .collect::<Vec<_>>();
                let resolved = resolve_install_graph(
                    &layout,
                    &backend,
                    &roots,
                    target.as_deref(),
                    &provider_overrides,
                )?;

                append_transaction_journal_entry(
                    &layout,
                    &tx.txid,
                    &TransactionJournalEntry {
                        seq: journal_seq,
                        step: "resolve_plan".to_string(),
                        state: "done".to_string(),
                        path: None,
                    },
                )?;
                journal_seq += 1;

                let planned_dependency_overrides = build_planned_dependency_overrides(&resolved);

                for package in &resolved {
                    let snapshot_path =
                        capture_package_state_snapshot(&layout, &tx.txid, &package.manifest.name)?;
                    append_transaction_journal_entry(
                        &layout,
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
                        &layout,
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

                    let dependencies = build_dependency_receipts(package, &resolved);
                    let outcome = install_resolved(
                        &layout,
                        package,
                        &dependencies,
                        &root_names,
                        &planned_dependency_overrides,
                        InstallResolvedOptions {
                            snapshot_id: snapshot_id.as_deref(),
                            force_redownload,
                            interaction_policy,
                        },
                    )?;
                    print_install_outcome(&outcome, output_style);
                }

                append_transaction_journal_entry(
                    &layout,
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
            if let Err(err) = sync_completion_assets_best_effort(&layout, "install") {
                eprintln!("{err}");
            }
        }
        Commands::Upgrade {
            spec,
            dry_run,
            provider,
            escalation,
        } => {
            let provider_overrides = parse_provider_overrides(&provider)?;
            let escalation_policy = resolve_escalation_policy(escalation);
            let interaction_policy = install_interaction_policy(escalation_policy);
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            run_upgrade_command(
                &layout,
                cli.registry_root.as_deref(),
                spec,
                dry_run,
                &provider_overrides,
                interaction_policy,
            )?;
        }
        Commands::Rollback { txid, escalation } => {
            let _escalation_policy = resolve_escalation_policy(escalation);
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            run_rollback_command(&layout, txid)?;
        }
        Commands::Repair { escalation } => {
            let _escalation_policy = resolve_escalation_policy(escalation);
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            run_repair_command(&layout)?;
        }
        Commands::Uninstall { name, escalation } => {
            let _escalation_policy = resolve_escalation_policy(escalation);
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            run_uninstall_command(&layout, name)?;
        }
        Commands::List => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            let receipts = read_install_receipts(&layout)?;
            if receipts.is_empty() {
                println!("No installed packages");
            } else {
                for receipt in receipts {
                    println!("{} {}", receipt.name, receipt.version);
                }
            }
        }
        Commands::Pin { spec } => {
            let (name, requirement) = parse_pin_spec(&spec)?;
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            layout.ensure_base_dirs()?;
            let pin_path = write_pin(&layout, &name, &requirement.to_string())?;
            println!("pinned {name} to {requirement}");
            println!("pin: {}", pin_path.display());
        }
        Commands::Registry { command } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            let source_state_root = registry_state_root(&layout);
            let store = RegistrySourceStore::new(&source_state_root);

            match command {
                RegistryCommands::Add {
                    name,
                    location,
                    kind,
                    priority,
                    fingerprint,
                } => {
                    let source_kind: RegistrySourceKind = kind.into();
                    let kind_label = format_registry_kind(source_kind.clone());
                    let output_lines =
                        format_registry_add_lines(&name, kind_label, priority, &fingerprint);
                    store.add_source(RegistrySourceRecord {
                        name,
                        kind: source_kind,
                        location,
                        fingerprint_sha256: fingerprint,
                        enabled: true,
                        priority,
                    })?;
                    for line in output_lines {
                        println!("{line}");
                    }
                }
                RegistryCommands::List => {
                    let sources = store.list_sources_with_snapshot_state()?;
                    for line in format_registry_list_lines(sources) {
                        println!("{line}");
                    }
                }
                RegistryCommands::Remove { name, purge_cache } => {
                    store.remove_source_with_cache_purge(&name, purge_cache)?;
                    for line in format_registry_remove_lines(&name, purge_cache) {
                        println!("{line}");
                    }
                }
            }
        }
        Commands::Update { registry } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            let source_state_root = registry_state_root(&layout);
            let store = RegistrySourceStore::new(&source_state_root);
            run_update_command(&store, &registry)?;
        }
        Commands::SelfUpdate {
            dry_run,
            force_redownload,
            escalation,
        } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            run_self_update_command(
                &layout,
                cli.registry_root.as_deref(),
                dry_run,
                force_redownload,
                escalation,
            )?;
        }
        Commands::Doctor => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            let output_style = current_output_style();
            println!(
                "{}",
                render_status_line(
                    output_style,
                    "step",
                    &format!("prefix: {}", layout.prefix().display())
                )
            );
            println!(
                "{}",
                render_status_line(
                    output_style,
                    "step",
                    &format!("bin: {}", layout.bin_dir().display())
                )
            );
            println!(
                "{}",
                render_status_line(
                    output_style,
                    "step",
                    &format!("cache: {}", layout.cache_dir().display())
                )
            );
            println!(
                "{}",
                render_status_line(
                    output_style,
                    "step",
                    &doctor_transaction_health_line(&layout)?
                )
            );
        }
        Commands::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
        }
        Commands::Completions { shell } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            let mut stdout = std::io::stdout();
            write_completions_script(shell, &layout, &mut stdout)?;
        }
        Commands::InitShell { shell } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            let resolved_shell =
                resolve_init_shell(shell, std::env::var("SHELL").ok().as_deref(), cfg!(windows));
            print_init_shell_snippet(&layout, resolved_shell);
        }
    }

    Ok(())
}
