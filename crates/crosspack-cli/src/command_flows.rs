fn ensure_upgrade_command_ready(layout: &PrefixLayout) -> Result<()> {
    layout.ensure_base_dirs()?;
    ensure_no_active_transaction_for(layout, "upgrade")
}

fn run_outdated_command(layout: &PrefixLayout, registry_root: Option<&Path>) -> Result<()> {
    let backend = select_metadata_backend(registry_root, layout)?;
    let receipts = read_install_receipts(layout)?;
    if receipts.is_empty() {
        println!("No installed packages");
        return Ok(());
    }

    let mut rows = Vec::new();
    for receipt in receipts {
        let installed_version = match Version::parse(&receipt.version) {
            Ok(version) => version,
            Err(_) => {
                rows.push(format!(
                    "{}\t{}\tunknown\tinvalid-installed-version",
                    receipt.name, receipt.version
                ));
                continue;
            }
        };

        let Some((source, manifests)) = backend.package_versions_with_source(&receipt.name)? else {
            continue;
        };
        let Some(latest) = manifests.first() else {
            continue;
        };

        if latest.version > installed_version {
            rows.push(format!(
                "{}\t{}\t{}\t{}",
                receipt.name, receipt.version, latest.version, source
            ));
        }
    }

    rows.sort();
    if rows.is_empty() {
        println!("All installed packages are up to date");
        return Ok(());
    }

    println!("name\tinstalled\tlatest\tsource");
    for row in rows {
        println!("{row}");
    }
    Ok(())
}

fn parse_receipt_dependency_name(entry: &str) -> Option<&str> {
    entry.split_once('@').map(|(name, _)| name)
}

fn ensure_installed_name_unambiguous(layout: &PrefixLayout, name: &str) -> Result<()> {
    resolve_installed_selector_for_cli(
        layout,
        &InstalledPackageSelector {
            package: name.to_string(),
            target: None,
            profile: None,
            source_namespace: None,
        },
    )
    .map(|_| ())
}

fn run_depends_command(layout: &PrefixLayout, name: &str) -> Result<()> {
    ensure_installed_name_unambiguous(layout, name)?;
    let receipts = read_install_receipts(layout)?;
    let Some(target) = receipts.iter().find(|receipt| receipt.name == name) else {
        println!("No installed package found: {name}");
        return Ok(());
    };

    let mut deps = target
        .dependencies
        .iter()
        .filter_map(|entry| parse_receipt_dependency_name(entry))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    deps.sort();
    deps.dedup();

    if deps.is_empty() {
        println!("{name} has no recorded dependencies");
        return Ok(());
    }

    println!("{name} dependency_count={}", deps.len());
    for dependency in deps {
        println!("dependency {dependency}");
    }
    Ok(())
}

fn run_uses_command(layout: &PrefixLayout, name: &str) -> Result<()> {
    let receipts = read_install_receipts(layout)?;
    let mut users = Vec::new();
    for receipt in receipts {
        if receipt
            .dependencies
            .iter()
            .filter_map(|entry| parse_receipt_dependency_name(entry))
            .any(|dependency_name| dependency_name == name)
        {
            users.push(receipt.name);
        }
    }

    users.sort();
    users.dedup();

    if users.is_empty() {
        println!("{name} is not required by any installed package");
        return Ok(());
    }

    println!("{name} reverse_dependency_count={}", users.len());
    for user in users {
        println!("required_by {user}");
    }
    Ok(())
}

fn run_why_command(layout: &PrefixLayout, name: &str) -> Result<()> {
    ensure_installed_name_unambiguous(layout, name)?;
    let receipts = read_install_receipts(layout)?;
    let receipt_map = receipts
        .iter()
        .map(|receipt| (receipt.name.clone(), receipt))
        .collect::<HashMap<_, _>>();
    let Some(target) = receipt_map.get(name) else {
        println!("No installed package found: {name}");
        return Ok(());
    };

    if target.install_reason == InstallReason::Root {
        println!("{name} is installed as a root package");
        return Ok(());
    }

    let mut roots = receipts
        .iter()
        .filter(|receipt| receipt.install_reason == InstallReason::Root)
        .map(|receipt| receipt.name.clone())
        .collect::<Vec<_>>();
    roots.sort();

    if let Some(path) = find_dependency_path_from_roots(name, &roots, &receipt_map) {
        println!("dependency path: {}", path.join(" -> "));
        return Ok(());
    }

    println!("no root dependency path found for {name}");
    Ok(())
}

fn find_dependency_path_from_roots(
    target: &str,
    roots: &[String],
    receipt_map: &HashMap<String, &InstallReceipt>,
) -> Option<Vec<String>> {
    let mut queue = std::collections::VecDeque::new();
    for root in roots {
        queue.push_back(vec![root.clone()]);
    }

    let mut visited = HashSet::new();
    while let Some(path) = queue.pop_front() {
        let current = path.last()?.clone();
        if current == target {
            return Some(path);
        }
        if !visited.insert(current.clone()) {
            continue;
        }

        let Some(receipt) = receipt_map.get(&current) else {
            continue;
        };
        let mut dependencies = receipt
            .dependencies
            .iter()
            .filter_map(|entry| parse_receipt_dependency_name(entry))
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        dependencies.sort();
        dependencies.dedup();

        for dependency in dependencies {
            let mut next_path = path.clone();
            next_path.push(dependency);
            queue.push_back(next_path);
        }
    }

    None
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ManagedServiceState {
    Stopped,
    Running,
}

impl ManagedServiceState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Running => "running",
        }
    }

    fn from_str(raw: &str) -> Option<Self> {
        match raw {
            "stopped" => Some(Self::Stopped),
            "running" => Some(Self::Running),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManagedServiceRow {
    package: String,
    name: String,
    state: ManagedServiceState,
    activation: Option<IntegrationActivationRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeclaredServiceRecord {
    package_state_key: String,
    package: String,
    service: ServiceDeclaration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectedIntegrationRow {
    package: String,
    name: String,
    key: String,
    kind: String,
    rel_path: String,
    activation: Option<IntegrationActivationRecord>,
}

fn managed_services_state_dir(layout: &PrefixLayout) -> PathBuf {
    layout.state_dir().join("services")
}

fn managed_service_state_path(layout: &PrefixLayout, name: &str) -> PathBuf {
    managed_services_state_dir(layout).join(format!("{name}.service"))
}

fn validate_service_name(name: &str) -> Result<()> {
    if !is_policy_token(name) {
        return Err(anyhow!(
            "invalid service name '{name}': use package-token grammar"
        ));
    }
    Ok(())
}

#[cfg(test)]
fn declared_service_for_name(layout: &PrefixLayout, name: &str) -> Result<DeclaredServiceRecord> {
    validate_service_name(name)?;
    let declared = collect_declared_services(layout)?;
    let Some(service) = declared.get(name).cloned() else {
        return Err(anyhow!(
            "No declared service found: {name}. Install or upgrade a package that declares this service in its manifest (for example: `crosspack install {name}`)"
        ));
    };
    Ok(service)
}

fn declared_service_for_package_service(
    layout: &PrefixLayout,
    package: &str,
    service: &str,
) -> Result<DeclaredServiceRecord> {
    validate_service_name(service)?;
    ensure_installed_name_unambiguous(layout, package)?;
    let declared_by_package = read_all_declared_services_states(layout)?;
    let Some(package_services) = declared_by_package.get(package) else {
        return Err(anyhow!(
            "No declared service found: package={package} service={service}"
        ));
    };
    let Some(declared) = package_services
        .iter()
        .find(|declared| declared.name == service)
        .cloned()
    else {
        return Err(anyhow!(
            "No declared service found: package={package} service={service}"
        ));
    };
    Ok(DeclaredServiceRecord {
        package_state_key: package_state_key_for_cli(layout, package)?,
        package: package.to_string(),
        service: declared,
    })
}

fn collect_declared_services(
    layout: &PrefixLayout,
) -> Result<HashMap<String, DeclaredServiceRecord>> {
    let receipts = read_install_receipts(layout)?;
    let declared_by_package = read_all_declared_services_states(layout)?;

    let mut services = HashMap::new();
    for receipt in &receipts {
        let Some(package_services) = declared_by_package.get(&receipt.name) else {
            continue;
        };
        for service in package_services {
            validate_service_name(&service.name)?;
            let existing = services.insert(
                service.name.clone(),
                DeclaredServiceRecord {
                    package_state_key: InstalledPackageIdentity::from_legacy_receipt(receipt)
                        .state_key(),
                    package: receipt.name.clone(),
                    service: service.clone(),
                },
            );
            if let Some(previous) = existing {
                return Err(anyhow!(
                    "duplicate declared service '{name}' across packages '{left}' and '{right}'",
                    name = service.name,
                    left = previous.package,
                    right = receipt.name
                ));
            }
        }
    }

    Ok(services)
}

#[cfg(test)]
fn declared_service_native_id(service: &ServiceDeclaration) -> String {
    service
        .native_id
        .clone()
        .unwrap_or_else(|| service.name.clone())
}

fn read_managed_service_state(layout: &PrefixLayout, name: &str) -> Result<ManagedServiceState> {
    validate_service_name(name)?;
    let path = managed_service_state_path(layout, name);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ManagedServiceState::Stopped);
        }
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed reading service state file: {}", path.display()));
        }
    };

    let mut parsed_state = None;
    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some(value) = line.strip_prefix("state=") else {
            return Err(anyhow!(
                "invalid service state file format: {}",
                path.display()
            ));
        };
        let Some(state) = ManagedServiceState::from_str(value) else {
            return Err(anyhow!(
                "invalid service state '{value}' in {}",
                path.display()
            ));
        };
        if parsed_state.is_some() {
            return Err(anyhow!(
                "duplicate service state entries in {}",
                path.display()
            ));
        }
        parsed_state = Some(state);
    }

    parsed_state.ok_or_else(|| anyhow!("missing service state in {}", path.display()))
}

#[cfg(test)]
fn write_managed_service_state(
    layout: &PrefixLayout,
    name: &str,
    state: ManagedServiceState,
) -> Result<PathBuf> {
    validate_service_name(name)?;
    let state_dir = managed_services_state_dir(layout);
    std::fs::create_dir_all(&state_dir).with_context(|| {
        format!(
            "failed creating service state directory: {}",
            state_dir.display()
        )
    })?;

    let path = managed_service_state_path(layout, name);
    std::fs::write(&path, format!("state={}\n", state.as_str()))
        .with_context(|| format!("failed writing service state file: {}", path.display()))?;
    Ok(path)
}

fn collect_managed_service_rows(layout: &PrefixLayout) -> Result<Vec<ManagedServiceRow>> {
    let declared = collect_declared_services(layout)?;
    let activation_records = read_integration_activation_state(layout)?;
    let mut rows = Vec::new();
    for (name, record) in declared {
        let integration_key = format!("service:{name}");
        let activation = activation_records
            .iter()
            .find(|activation| {
                activation.package_state_key == record.package_state_key
                    && activation.integration_key == integration_key
            })
            .cloned();
        rows.push(ManagedServiceRow {
            package: record.package,
            name: name.clone(),
            state: read_managed_service_state(layout, &name)?,
            activation,
        });
    }

    rows.sort_by(|left, right| {
        left.package
            .cmp(&right.package)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(rows)
}

fn run_service_status_for_package_command(
    layout: &PrefixLayout,
    package: &str,
    service: &str,
) -> Result<()> {
    let mut executor = SystemActivationCommandExecutor;
    let line = service_status_line_for_package(layout, package, service, |plan| {
        run_service_action_plan(&mut executor, plan, NativeServiceAction::Status)
    })?;
    println!("{line}");
    Ok(())
}

fn service_status_line_for_package(
    layout: &PrefixLayout,
    package: &str,
    service: &str,
    mut run_status: impl FnMut(&IntegrationActivationPlan) -> ActivationAdapterOutcome,
) -> Result<String> {
    declared_service_for_package_service(layout, package, service)?;
    match service_activation_record(layout, package, service)? {
        Some(mut activation) => {
            let plan = activation_plan_from_record(&activation)?;
            let outcome = run_status(&plan);
            activation.applied_state = outcome.applied_state;
            activation.reason_code = outcome.reason_code;
            Ok(format_service_activation_line(
                package,
                service,
                &activation,
                activation.reason_code == IntegrationReasonCode::Ok,
            ))
        }
        None => Ok(format_projected_service_line(package, service)),
    }
}

#[cfg(test)]
fn service_status_line_for_package_from_state(
    layout: &PrefixLayout,
    package: &str,
    service: &str,
) -> Result<String> {
    service_status_line_for_package(layout, package, service, |_| {
        service_activation_record(layout, package, service)
            .ok()
            .flatten()
            .map(|activation| ActivationAdapterOutcome {
                reason_code: activation.reason_code,
                applied_state: activation.applied_state,
                rollback: Vec::new(),
            })
            .unwrap_or_else(|| ActivationAdapterOutcome {
                reason_code: IntegrationReasonCode::StateMissing,
                applied_state: IntegrationAppliedState::Unsupported,
                rollback: Vec::new(),
            })
    })
}

fn run_service_action_for_package_command(
    layout: &PrefixLayout,
    package: &str,
    service: &str,
    action: NativeServiceAction,
) -> Result<()> {
    ensure_no_active_transaction_for(layout, "services")?;
    let mut executor = SystemActivationCommandExecutor;
    let line = service_action_line_for_package(layout, package, service, action, |plan| {
        run_service_action_plan(&mut executor, plan, action)
    })?;
    println!("{line}");
    Ok(())
}

fn service_action_line_for_package(
    layout: &PrefixLayout,
    package: &str,
    service: &str,
    action: NativeServiceAction,
    mut run_action: impl FnMut(&IntegrationActivationPlan) -> ActivationAdapterOutcome,
) -> Result<String> {
    declared_service_for_package_service(layout, package, service)?;
    let Some(mut activation) = service_activation_record(layout, package, service)? else {
        let activation = IntegrationActivationRecord {
            package_state_key: package.to_string(),
            package: package.to_string(),
            integration_key: format!("service:{service}"),
            kind: "service".to_string(),
            adapter: IntegrationAdapterKind::None,
            scope: IntegrationActivationScope::User,
            desired_state: IntegrationDesiredState::Projected,
            applied_state: IntegrationAppliedState::Unsupported,
            host_path: None,
            reason_code: IntegrationReasonCode::StateMissing,
        };
        return Ok(format_service_activation_line(package, service, &activation, false));
    };
    let plan = activation_plan_from_record(&activation)?;
    let outcome = run_action(&plan);
    activation.applied_state = outcome.applied_state;
    activation.reason_code = outcome.reason_code;
    activation.desired_state = match action {
        NativeServiceAction::Stop => IntegrationDesiredState::Projected,
        NativeServiceAction::Status => activation.desired_state,
        NativeServiceAction::Start | NativeServiceAction::Restart => IntegrationDesiredState::Running,
    };
    if action != NativeServiceAction::Status {
        upsert_activation_record(layout, activation.clone())?;
    }
    Ok(format_service_activation_line(
        package,
        service,
        &activation,
        activation.reason_code == IntegrationReasonCode::Ok,
    ))
}

#[cfg(test)]
fn run_service_start_command(layout: &PrefixLayout, name: &str) -> Result<()> {
    ensure_installed_name_unambiguous(layout, name)?;
    let declared = declared_service_for_name(layout, name)?;
    let native_outcome = run_native_service_action(
        NativeServiceAction::Start,
        &declared.service.name,
        &declared_service_native_id(&declared.service),
    );
    let next_state = if native_outcome.applied {
        ManagedServiceState::Running
    } else {
        read_managed_service_state(layout, name)?
    };
    if native_outcome.applied {
        write_managed_service_state(layout, name, next_state)?;
    }
    println!(
        "{}",
        render_service_state_line(name, next_state, Some("start"), &native_outcome)
    );
    Ok(())
}

#[cfg(test)]
fn run_service_stop_command(layout: &PrefixLayout, name: &str) -> Result<()> {
    ensure_installed_name_unambiguous(layout, name)?;
    let declared = declared_service_for_name(layout, name)?;
    let native_outcome = run_native_service_action(
        NativeServiceAction::Stop,
        &declared.service.name,
        &declared_service_native_id(&declared.service),
    );
    let next_state = if native_outcome.applied {
        ManagedServiceState::Stopped
    } else {
        read_managed_service_state(layout, name)?
    };
    if native_outcome.applied {
        write_managed_service_state(layout, name, next_state)?;
    }
    println!(
        "{}",
        render_service_state_line(name, next_state, Some("stop"), &native_outcome)
    );
    Ok(())
}

#[cfg(test)]
fn run_service_restart_command(layout: &PrefixLayout, name: &str) -> Result<()> {
    ensure_installed_name_unambiguous(layout, name)?;
    let declared = declared_service_for_name(layout, name)?;
    let native_outcome = run_native_service_action(
        NativeServiceAction::Restart,
        &declared.service.name,
        &declared_service_native_id(&declared.service),
    );
    let next_state = if native_outcome.applied {
        ManagedServiceState::Running
    } else {
        read_managed_service_state(layout, name)?
    };
    if native_outcome.applied {
        write_managed_service_state(layout, name, next_state)?;
    }
    println!(
        "{}",
        render_service_state_line(name, next_state, Some("restart"), &native_outcome)
    );
    Ok(())
}

#[cfg(test)]
fn render_service_state_line(
    name: &str,
    state: ManagedServiceState,
    action: Option<&str>,
    native_outcome: &NativeServiceOutcome,
) -> String {
    let mut line = format!("service_state name={name} state={}", state.as_str());
    if let Some(action) = action {
        line.push_str(&format!(" action={action}"));
    }
    line.push_str(&format!(
        " adapter={} applied={} reason={}",
        native_outcome.adapter, native_outcome.applied, native_outcome.reason_code
    ));
    line
}

fn format_projected_service_line(package: &str, service: &str) -> String {
    format!(
        "service package={package} name={service} state=projected adapter=none scope=user applied=false reason=not-enabled"
    )
}

fn format_service_list_projection_line(
    package: &str,
    service: &str,
    state: ManagedServiceState,
) -> String {
    format!(
        "service package={package} name={service} state={} adapter=none scope=user applied=false reason=not-enabled",
        state.as_str()
    )
}

fn format_managed_service_row(row: &ManagedServiceRow) -> String {
    if let Some(activation) = &row.activation {
        format_service_activation_line(
            &row.package,
            &row.name,
            activation,
            service_activation_applied(activation),
        )
    } else {
        format_service_list_projection_line(&row.package, &row.name, row.state)
    }
}

fn format_service_activation_line(
    package: &str,
    service: &str,
    activation: &IntegrationActivationRecord,
    applied: bool,
) -> String {
    format!(
        "service package={package} name={service} state={} adapter={} scope={} applied={} reason={}",
        activation.applied_state.as_str(),
        activation.adapter.as_str(),
        activation.scope.as_str(),
        applied,
        activation.reason_code.as_str()
    )
}

fn service_activation_applied(activation: &IntegrationActivationRecord) -> bool {
    activation.reason_code == IntegrationReasonCode::Ok
        && matches!(
            activation.applied_state,
            IntegrationAppliedState::Installed
                | IntegrationAppliedState::Enabled
                | IntegrationAppliedState::Running
        )
}

fn service_activation_record(
    layout: &PrefixLayout,
    package: &str,
    service: &str,
) -> Result<Option<IntegrationActivationRecord>> {
    let key = format!("service:{service}");
    let matches = read_integration_activation_state(layout)?
        .into_iter()
        .filter(|record| record.package == package && record.integration_key == key)
        .collect::<Vec<_>>();
    if matches.len() > 1 {
        return Err(anyhow!(
            "ambiguous service activation state: package={package} service={service}"
        ));
    }
    Ok(matches.into_iter().next())
}

fn run_services_command(layout: &PrefixLayout, command: ServicesCommands) -> Result<()> {
    layout.ensure_base_dirs()?;
    match command {
        ServicesCommands::List => {
            let rows = collect_managed_service_rows(layout)?;
            if rows.is_empty() {
                println!("No managed services");
            } else {
                for row in rows {
                    println!("{}", format_managed_service_row(&row));
                }
            }
        }
        ServicesCommands::Status { package, service } => {
            run_service_status_for_package_command(layout, &package, &service)?
        }
        ServicesCommands::Start { package, service } => run_service_action_for_package_command(
            layout,
            &package,
            &service,
            NativeServiceAction::Start,
        )?,
        ServicesCommands::Stop { package, service } => run_service_action_for_package_command(
            layout,
            &package,
            &service,
            NativeServiceAction::Stop,
        )?,
        ServicesCommands::Restart { package, service } => run_service_action_for_package_command(
            layout,
            &package,
            &service,
            NativeServiceAction::Restart,
        )?,
    }
    Ok(())
}

fn projected_integration_short_name(key: &str) -> &str {
    key.rsplit(':').next().unwrap_or(key)
}

fn row_from_projected_integration(
    package: &str,
    projection: &IntegrationProjection,
) -> ProjectedIntegrationRow {
    ProjectedIntegrationRow {
        package: package.to_string(),
        name: projected_integration_short_name(&projection.key).to_string(),
        key: projection.key.clone(),
        kind: projection.kind.clone(),
        rel_path: projection.rel_path.clone(),
        activation: None,
    }
}

fn collect_projected_integration_rows(layout: &PrefixLayout) -> Result<Vec<ProjectedIntegrationRow>> {
    let states = read_all_integration_states(layout)?;
    let activations = read_integration_activation_state(layout)?;
    let mut rows = Vec::new();
    let mut seen_logical_integrations = std::collections::BTreeSet::new();
    let host_platform = current_host_platform();
    for (package, projections) in states {
        let mut service_candidates = std::collections::BTreeMap::<
            String,
            Vec<IntegrationProjection>,
        >::new();
        let mut non_service_projections = Vec::new();
        for projection in projections {
            if projection.kind == "service" {
                service_candidates
                    .entry(projection.key.clone())
                    .or_default()
                    .push(projection);
            } else {
                non_service_projections.push(projection);
            }
        }

        let selected_service_projections = service_candidates
            .into_values()
            .filter_map(|mut candidates| select_host_service_projection(&mut candidates, host_platform))
            .collect::<Vec<_>>();

        for projection in non_service_projections
            .into_iter()
            .chain(selected_service_projections)
        {
            if projection.kind == "service"
                && !seen_logical_integrations.insert((package.clone(), projection.key.clone()))
            {
                continue;
            }
            let mut row = row_from_projected_integration(&package, &projection);
            row.activation = activations
                .iter()
                .find(|activation| {
                    activation.package == package && activation.integration_key == projection.key
                })
                .cloned();
            rows.push(row);
        }
    }
    rows.sort_by(|left, right| {
        left.package
            .cmp(&right.package)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.key.cmp(&right.key))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.rel_path.cmp(&right.rel_path))
    });
    Ok(rows)
}

fn select_host_service_projection(
    candidates: &mut Vec<IntegrationProjection>,
    platform: HostPlatform,
) -> Option<IntegrationProjection> {
    let host_match = candidates
        .iter()
        .position(|projection| service_projection_matches_host(projection, platform));
    if let Some(index) = host_match {
        return Some(candidates.remove(index));
    }
    candidates.sort_by(|left, right| left.rel_path.cmp(&right.rel_path));
    if candidates.is_empty() {
        None
    } else {
        Some(candidates.remove(0))
    }
}

fn service_projection_matches_host(projection: &IntegrationProjection, platform: HostPlatform) -> bool {
    match platform {
        HostPlatform::Linux => projection.rel_path.ends_with(".service"),
        HostPlatform::Macos => projection.rel_path.ends_with(".launchd.plist"),
        HostPlatform::Windows => projection.rel_path.ends_with(".windows-service.toml"),
    }
}

fn format_projected_integration_line(row: &ProjectedIntegrationRow) -> String {
    if let Some(activation) = &row.activation {
        return format_integration_activation_row(row, activation);
    }
    format!(
        "integration package={} name={} key={} kind={} state=projected adapter=none reason=not-enabled path={}",
        row.package,
        row.name,
        row.key,
        row.kind,
        encode_output_value(&row.rel_path)
    )
}

fn format_integration_activation_line(
    package: &str,
    projection: &IntegrationProjection,
    activation: Option<&IntegrationActivationRecord>,
) -> String {
    let row = row_from_projected_integration(package, projection);
    match activation {
        Some(activation) => format_integration_activation_row(&row, activation),
        None => format_projected_integration_line(&row),
    }
}

fn format_integration_activation_row(
    row: &ProjectedIntegrationRow,
    activation: &IntegrationActivationRecord,
) -> String {
    format!(
        "integration package={} name={} key={} kind={} state={} adapter={} reason={} path={}",
        row.package,
        row.name,
        row.key,
        row.kind,
        integration_output_state(activation).as_str(),
        activation.adapter.as_str(),
        activation.reason_code.as_str(),
        encode_output_value(activation.host_path.as_deref().unwrap_or(&row.rel_path))
    )
}

fn encode_output_value(value: &str) -> String {
    let mut encoded = String::new();
    for ch in value.chars() {
        match ch {
            '%' => encoded.push_str("%25"),
            ' ' => encoded.push_str("%20"),
            '\\' => encoded.push_str("%5C"),
            '\t' => encoded.push_str("%09"),
            '\n' => encoded.push_str("%0A"),
            '\r' => encoded.push_str("%0D"),
            _ if ch.is_control() => {
                let mut buffer = [0; 4];
                for byte in ch.encode_utf8(&mut buffer).as_bytes() {
                    encoded.push_str(&format!("%{byte:02X}"));
                }
            }
            _ => encoded.push(ch),
        }
    }
    encoded
}

fn integration_output_state(activation: &IntegrationActivationRecord) -> IntegrationAppliedState {
    if activation.desired_state == IntegrationDesiredState::Projected
        || activation.desired_state == IntegrationDesiredState::Disabled
        || activation.reason_code != IntegrationReasonCode::Ok
    {
        IntegrationAppliedState::Projected
    } else {
        activation.applied_state.clone()
    }
}

fn format_projected_integration_lines(rows: &[ProjectedIntegrationRow]) -> Vec<String> {
    rows.iter().map(format_projected_integration_line).collect()
}

fn integration_status_line(
    layout: &PrefixLayout,
    package: &str,
    integration: &str,
) -> Result<String> {
    let projection = resolve_projected_integration(layout, package, integration)?;
    let activation = read_integration_activation_state(layout)?
        .into_iter()
        .find(|record| record.package == package && record.integration_key == projection.key);
    Ok(format_integration_activation_line(
        package,
        &projection,
        activation.as_ref(),
    ))
}

fn resolve_projected_integration(
    layout: &PrefixLayout,
    package: &str,
    integration: &str,
) -> Result<IntegrationProjection> {
    let projections = read_integration_state(layout, package)?;
    let mut matches = projections
        .into_iter()
        .filter(|projection| {
            projection.key == integration
                || projected_integration_short_name(&projection.key) == integration
        })
        .collect::<Vec<_>>();

    if matches.is_empty() {
        return Err(anyhow!(
            "No projected integration found: package={package} integration={integration}"
        ));
    }
    if matches.len() > 1 {
        let first_key = matches[0].key.clone();
        let first_kind = matches[0].kind.clone();
        if matches
            .iter()
            .all(|projection| projection.key == first_key && projection.kind == first_kind)
        {
            if first_kind == "service" {
                if let Some(projection) =
                    select_host_service_projection(&mut matches, current_host_platform())
                {
                    return Ok(projection);
                }
            }
            matches.sort_by(|left, right| left.rel_path.cmp(&right.rel_path));
            return Ok(matches.remove(0));
        }
        return Err(anyhow!(
            "ambiguous projected integration: package={package} integration={integration}; use full integration key"
        ));
    }

    Ok(matches.remove(0))
}

fn run_integrations_command(layout: &PrefixLayout, command: IntegrationsCommands) -> Result<()> {
    match command {
        IntegrationsCommands::List => {
            let rows = collect_projected_integration_rows(layout)?;
            if rows.is_empty() {
                println!("No projected integrations");
            } else {
                for line in format_projected_integration_lines(&rows) {
                    println!("{line}");
                }
            }
        }
        IntegrationsCommands::Status {
            package,
            integration,
        } => println!("{}", integration_status_line(layout, &package, &integration)?),
        IntegrationsCommands::Enable {
            package,
            integration,
        } => println!(
            "{}",
            run_integration_activation_command(layout, &package, &integration, true)?
        ),
        IntegrationsCommands::Disable {
            package,
            integration,
        } => println!(
            "{}",
            run_integration_activation_command(layout, &package, &integration, false)?
        ),
    }
    Ok(())
}

fn run_integration_activation_command(
    layout: &PrefixLayout,
    package: &str,
    integration: &str,
    enable: bool,
) -> Result<String> {
    ensure_no_active_transaction_for(layout, "integrations")?;
    let mut line = None;
    execute_with_transaction(layout, "integrations", None, |tx| {
        line = Some(run_integration_activation_command_inner(
            layout,
            Some(tx),
            package,
            integration,
            enable,
        )?);
        Ok(())
    })?;
    line.ok_or_else(|| anyhow!("integration activation did not produce output"))
}

fn run_integration_activation_command_inner(
    layout: &PrefixLayout,
    tx: Option<&TransactionMetadata>,
    package: &str,
    integration: &str,
    enable: bool,
) -> Result<String> {
    layout.ensure_base_dirs()?;
    ensure_installed_name_unambiguous(layout, package)?;
    let projection = resolve_projected_integration(layout, package, integration)?;
    let host = current_host_activation_context(layout)?;
    let mut plan = plan_activation_for_projection(&host, package, &projection)?;
    plan.package_state_key = package_state_key_for_cli(layout, package)?;
    let records = read_integration_activation_state(layout)?;
    let mut fs = real_activation_fs_from_records(host.platform, &records);
    let rollback = preview_integration_activation_rollback(&fs, &plan, enable);
    journal_integration_activation_rollback_payload(layout, tx, rollback.as_ref())?;
    let outcome = if plan.kind == "service" {
        let mut executor = SystemActivationCommandExecutor;
        if enable {
            apply_service_plan(&mut executor, &plan)
        } else {
            disable_service_plan(&mut executor, &plan)
        }
    } else if enable {
        apply_integration_plan_with_fs(&mut fs, &plan)
    } else {
        disable_integration_plan_with_fs(&mut fs, &plan)
    };
    finish_integration_activation_command(layout, package, &projection, &plan, outcome, enable)
}

#[cfg(test)]
fn run_integration_activation_command_with_fs(
    layout: &PrefixLayout,
    host: &HostActivationContext,
    fs: &mut impl ActivationFilesystem,
    package: &str,
    integration: &str,
    enable: bool,
) -> Result<String> {
    run_integration_activation_command_with_fs_and_tx(
        layout,
        None,
        host,
        fs,
        package,
        integration,
        enable,
    )
}

#[cfg(test)]
fn run_integration_activation_command_with_fs_and_tx(
    layout: &PrefixLayout,
    tx: Option<&TransactionMetadata>,
    host: &HostActivationContext,
    fs: &mut impl ActivationFilesystem,
    package: &str,
    integration: &str,
    enable: bool,
) -> Result<String> {
    run_integration_activation_command_with_fs_tx_and_service_runner(
        layout,
        tx,
        host,
        fs,
        package,
        integration,
        enable,
        |plan, enable| {
            let mut executor = SystemActivationCommandExecutor;
            if enable {
                apply_service_plan(&mut executor, plan)
            } else {
                disable_service_plan(&mut executor, plan)
            }
        },
    )
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn run_integration_activation_command_with_fs_tx_and_service_runner(
    layout: &PrefixLayout,
    tx: Option<&TransactionMetadata>,
    host: &HostActivationContext,
    fs: &mut impl ActivationFilesystem,
    package: &str,
    integration: &str,
    enable: bool,
    mut run_service: impl FnMut(&IntegrationActivationPlan, bool) -> ActivationAdapterOutcome,
) -> Result<String> {
    layout.ensure_base_dirs()?;
    ensure_installed_name_unambiguous(layout, package)?;
    let projection = resolve_projected_integration(layout, package, integration)?;
    let mut plan = plan_activation_for_projection(host, package, &projection)?;
    plan.package_state_key = package_state_key_for_cli(layout, package)?;
    let rollback = preview_integration_activation_rollback(fs, &plan, enable);
    journal_integration_activation_rollback_payload(layout, tx, rollback.as_ref())?;
    let outcome = if plan.kind == "service" {
        run_service(&plan, enable)
    } else if enable {
        apply_integration_plan_with_fs(fs, &plan)
    } else {
        disable_integration_plan_with_fs(fs, &plan)
    };
    finish_integration_activation_command(layout, package, &projection, &plan, outcome, enable)
}

fn journal_integration_activation_rollback_payload(
    layout: &PrefixLayout,
    tx: Option<&TransactionMetadata>,
    rollback: Option<&ActivationRollbackEntry>,
) -> Result<()> {
    let Some(tx) = tx else {
        return Ok(());
    };
    let Some(rollback) = rollback else {
        return Ok(());
    };
    append_transaction_journal_entry(
        layout,
        &tx.txid,
        &TransactionJournalEntry {
            seq: 1,
            step: "integration_activation_rollback".to_string(),
            state: "planned".to_string(),
            path: Some(serde_json::to_string(rollback)?),
        },
    )
    .map(|_| ())
}

fn preview_integration_activation_rollback(
    fs: &impl ActivationFilesystem,
    plan: &IntegrationActivationPlan,
    enable: bool,
) -> Option<ActivationRollbackEntry> {
    let expected_owner = ActivationOwner {
        package_state_key: plan.package_state_key.clone(),
        package: plan.package.clone(),
        integration_key: plan.integration_key.clone(),
    };
    match (plan.kind.as_str(), enable, fs.platform(), fs.entry(&plan.host_path)) {
        ("docker_cli_plugin", true, _, None) => Some(ActivationRollbackEntry {
            operation: ActivationRollbackOperation::RemoveCreatedSymlink,
            path: plan.host_path.clone(),
            previous_symlink_target: None,
            previous_shim_target: None,
            previous_owner: None,
            created_symlink_target: Some(plan.source_path.clone()),
            created_shim_target: None,
            created_owner: Some(expected_owner.clone()),
            expected_current_symlink_target: None,
            expected_current_shim_target: None,
            expected_current_owner: None,
            expected_current_absent: false,
            created_parent_dirs: Vec::new(),
        }),
        ("docker_cli_plugin", true, _, Some(ActivationFsEntry::Symlink { target, owner }))
            if owner.as_ref() == Some(&expected_owner) && target != plan.source_path =>
        {
            Some(ActivationRollbackEntry {
                operation: ActivationRollbackOperation::RestoreOwnedSymlink,
                path: plan.host_path.clone(),
                previous_symlink_target: Some(target),
                previous_shim_target: None,
                previous_owner: owner,
                created_symlink_target: None,
                created_shim_target: None,
                created_owner: None,
                expected_current_symlink_target: Some(plan.source_path.clone()),
                expected_current_shim_target: None,
                expected_current_owner: Some(expected_owner.clone()),
                expected_current_absent: false,
                created_parent_dirs: Vec::new(),
            })
        }
        ("docker_cli_plugin", false, _, Some(ActivationFsEntry::Symlink { target, owner }))
            if target == plan.source_path && owner.as_ref() == Some(&expected_owner) =>
        {
            Some(ActivationRollbackEntry {
                operation: ActivationRollbackOperation::RestoreOwnedSymlink,
                path: plan.host_path.clone(),
                previous_symlink_target: Some(target),
                previous_shim_target: None,
                previous_owner: owner,
                created_symlink_target: None,
                created_shim_target: None,
                created_owner: None,
                expected_current_symlink_target: None,
                expected_current_shim_target: None,
                expected_current_owner: None,
                expected_current_absent: true,
                created_parent_dirs: Vec::new(),
            })
        }
        ("path_plugin", true, HostPlatform::Linux | HostPlatform::Macos, None) => {
            Some(ActivationRollbackEntry {
                operation: ActivationRollbackOperation::RemoveCreatedSymlink,
                path: plan.host_path.clone(),
                previous_symlink_target: None,
                previous_shim_target: None,
                previous_owner: None,
                created_symlink_target: Some(plan.source_path.clone()),
                created_shim_target: None,
                created_owner: Some(expected_owner.clone()),
                expected_current_symlink_target: None,
                expected_current_shim_target: None,
                expected_current_owner: None,
                expected_current_absent: false,
                created_parent_dirs: Vec::new(),
            })
        }
        (
            "path_plugin",
            true,
            HostPlatform::Linux | HostPlatform::Macos,
            Some(ActivationFsEntry::Symlink { target, owner }),
        ) if owner.as_ref() == Some(&expected_owner) && target != plan.source_path => {
            Some(ActivationRollbackEntry {
                operation: ActivationRollbackOperation::RestoreOwnedSymlink,
                path: plan.host_path.clone(),
                previous_symlink_target: Some(target),
                previous_shim_target: None,
                previous_owner: owner,
                created_symlink_target: None,
                created_shim_target: None,
                created_owner: None,
                expected_current_symlink_target: Some(plan.source_path.clone()),
                expected_current_shim_target: None,
                expected_current_owner: Some(expected_owner.clone()),
                expected_current_absent: false,
                created_parent_dirs: Vec::new(),
            })
        }
        ("path_plugin", true, HostPlatform::Windows, None) => Some(ActivationRollbackEntry {
            operation: ActivationRollbackOperation::RemoveCreatedWindowsShim,
            path: plan.host_path.clone(),
            previous_symlink_target: None,
            previous_shim_target: None,
            previous_owner: None,
            created_symlink_target: None,
            created_shim_target: Some(plan.source_path.clone()),
            created_owner: Some(expected_owner.clone()),
            expected_current_symlink_target: None,
            expected_current_shim_target: None,
            expected_current_owner: None,
            expected_current_absent: false,
            created_parent_dirs: Vec::new(),
        }),
        (
            "path_plugin",
            true,
            HostPlatform::Windows,
            Some(ActivationFsEntry::WindowsShim { target, owner }),
        ) if owner.as_ref() == Some(&expected_owner) && target != plan.source_path => {
            Some(ActivationRollbackEntry {
                operation: ActivationRollbackOperation::RestoreOwnedWindowsShim,
                path: plan.host_path.clone(),
                previous_symlink_target: None,
                previous_shim_target: Some(target),
                previous_owner: owner,
                created_symlink_target: None,
                created_shim_target: None,
                created_owner: None,
                expected_current_symlink_target: None,
                expected_current_shim_target: Some(plan.source_path.clone()),
                expected_current_owner: Some(expected_owner.clone()),
                expected_current_absent: false,
                created_parent_dirs: Vec::new(),
            })
        }
        (
            "path_plugin",
            false,
            HostPlatform::Linux | HostPlatform::Macos,
            Some(ActivationFsEntry::Symlink { target, owner }),
        ) if target == plan.source_path && owner.as_ref() == Some(&expected_owner) => {
            Some(ActivationRollbackEntry {
                operation: ActivationRollbackOperation::RestoreOwnedSymlink,
                path: plan.host_path.clone(),
                previous_symlink_target: Some(target),
                previous_shim_target: None,
                previous_owner: owner,
                created_symlink_target: None,
                created_shim_target: None,
                created_owner: None,
                expected_current_symlink_target: None,
                expected_current_shim_target: None,
                expected_current_owner: None,
                expected_current_absent: true,
                created_parent_dirs: Vec::new(),
            })
        }
        (
            "path_plugin",
            false,
            HostPlatform::Windows,
            Some(ActivationFsEntry::WindowsShim { target, owner }),
        ) if target == plan.source_path && owner.as_ref() == Some(&expected_owner) => {
            Some(ActivationRollbackEntry {
                operation: ActivationRollbackOperation::RestoreOwnedWindowsShim,
                path: plan.host_path.clone(),
                previous_symlink_target: None,
                previous_shim_target: Some(target),
                previous_owner: owner,
                created_symlink_target: None,
                created_shim_target: None,
                created_owner: None,
                expected_current_symlink_target: None,
                expected_current_shim_target: None,
                expected_current_owner: None,
                expected_current_absent: true,
                created_parent_dirs: Vec::new(),
            })
        }
        _ => None,
    }
}

fn real_activation_fs_from_records(
    platform: HostPlatform,
    records: &[IntegrationActivationRecord],
) -> RealActivationFs {
    RealActivationFs::new(
        platform,
        records
            .iter()
            .filter(|record| {
                record.reason_code == IntegrationReasonCode::Ok
                    && matches!(
                        record.applied_state,
                        IntegrationAppliedState::Installed
                            | IntegrationAppliedState::Enabled
                            | IntegrationAppliedState::Running
                    )
            })
            .filter_map(|record| {
            Some((
                record.host_path.clone()?,
                ActivationOwner {
                    package_state_key: record.package_state_key.clone(),
                    package: record.package.clone(),
                    integration_key: record.integration_key.clone(),
                },
            ))
        }),
    )
}

fn finish_integration_activation_command(
    layout: &PrefixLayout,
    package: &str,
    projection: &IntegrationProjection,
    plan: &IntegrationActivationPlan,
    outcome: ActivationAdapterOutcome,
    enable: bool,
) -> Result<String> {
    let record = IntegrationActivationRecord {
        package_state_key: plan.package_state_key.clone(),
        package: package.to_string(),
        integration_key: projection.key.clone(),
        kind: projection.kind.clone(),
        adapter: plan.adapter.clone(),
        scope: plan.scope.clone(),
        desired_state: if enable {
            IntegrationDesiredState::Enabled
        } else {
            IntegrationDesiredState::Projected
        },
        applied_state: if enable {
            outcome.applied_state
        } else if outcome.reason_code == IntegrationReasonCode::Ok {
            IntegrationAppliedState::Projected
        } else {
            outcome.applied_state
        },
        host_path: Some(plan.host_path.clone()),
        reason_code: outcome.reason_code,
    };
    upsert_activation_record(layout, record.clone())?;
    Ok(format_integration_activation_line(
        package,
        projection,
        Some(&record),
    ))
}

fn plan_activation_for_projection(
    host: &HostActivationContext,
    package: &str,
    projection: &IntegrationProjection,
) -> Result<IntegrationActivationPlan> {
    match projection.kind.as_str() {
        "docker_cli_plugin" => plan_docker_cli_plugin_activation(host, package, projection)
            .map_err(|err| anyhow!(err)),
        "path_plugin" => {
            let host_name = path_plugin_host_binary_name(&projection.key)?;
            plan_path_plugin_activation(host, package, &host_name, projection)
                .map_err(|err| anyhow!(err))
        }
        "service" => {
            let metadata = service_activation_metadata_from_projection(projection)?;
            plan_service_activation(host, package, &metadata).map_err(|err| anyhow!(err))
        }
        kind => Err(anyhow!(
            "integration activation is not supported for kind '{kind}'"
        )),
    }
}

fn service_activation_metadata_from_projection(
    projection: &IntegrationProjection,
) -> Result<ServiceActivationMetadata> {
    let name = projected_integration_short_name(&projection.key);
    let mut metadata = ServiceActivationMetadata::new(name);
    if projection.rel_path.ends_with(".service") {
        metadata = metadata.with_source(&projection.rel_path);
    } else if projection.rel_path.ends_with(".launchd.plist") {
        metadata = metadata.with_macos_launch_agent(&projection.rel_path);
    } else if projection.rel_path.ends_with(".windows-service.toml") {
        metadata = metadata.with_windows_service(&projection.rel_path);
    } else {
        return Err(anyhow!(
            "invalid service integration projection path: {}",
            projection.rel_path
        ));
    }
    Ok(metadata)
}

fn path_plugin_host_binary_name(key: &str) -> Result<String> {
    let mut parts = key.split(':');
    let kind = parts.next();
    let host = parts.next();
    let name = parts.next();
    if kind != Some("path_plugin") || host.is_none() || name.is_none() || parts.next().is_some() {
        return Err(anyhow!("invalid path plugin integration key: {key}"));
    }
    Ok(format!("{}-{}", host.unwrap(), name.unwrap()))
}

fn current_host_activation_context(layout: &PrefixLayout) -> Result<HostActivationContext> {
    let platform = current_host_platform();
    let mut host = match platform {
        HostPlatform::Linux => HostActivationContext::linux(),
        HostPlatform::Macos => HostActivationContext::macos(),
        HostPlatform::Windows => HostActivationContext::windows(),
    }
    .with_prefix(&layout.prefix().display().to_string());

    for key in ["DOCKER_CONFIG"] {
        if let Ok(value) = std::env::var(key) {
            host = host.with_env(key, &value);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        host = host.with_home(&home);
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        host = host.with_user_profile(&profile);
    }
    Ok(host)
}

fn current_host_platform() -> HostPlatform {
    if cfg!(target_os = "windows") {
        HostPlatform::Windows
    } else if cfg!(target_os = "macos") {
        HostPlatform::Macos
    } else {
        HostPlatform::Linux
    }
}

fn package_state_key_for_cli(layout: &PrefixLayout, package: &str) -> Result<String> {
    let states = read_all_installed_package_states(layout)?;
    let matches = states
        .into_iter()
        .filter(|state| state.identity.package == package)
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        Ok(matches[0].identity.state_key())
    } else {
        Ok(package.to_string())
    }
}

fn upsert_activation_record(layout: &PrefixLayout, record: IntegrationActivationRecord) -> Result<()> {
    let mut records = read_integration_activation_state(layout)?;
    records.retain(|existing| {
        !(existing.package_state_key == record.package_state_key
            && existing.integration_key == record.integration_key)
    });
    records.push(record);
    write_integration_activation_state(layout, &records).map(|_| ())
}

fn activation_plan_from_record(record: &IntegrationActivationRecord) -> Result<IntegrationActivationPlan> {
    Ok(IntegrationActivationPlan {
        package_state_key: record.package_state_key.clone(),
        package: record.package.clone(),
        integration_key: record.integration_key.clone(),
        kind: record.kind.clone(),
        adapter: record.adapter.clone(),
        scope: record.scope.clone(),
        desired_state: record.desired_state.clone(),
        host_path: record
            .host_path
            .clone()
            .ok_or_else(|| anyhow!("activation record missing host path"))?,
        source_path: String::new(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CacheFileEntry {
    path: PathBuf,
    size: u64,
}

fn run_cache_command(layout: &PrefixLayout, command: CacheCommands) -> Result<()> {
    layout.ensure_base_dirs()?;
    match command {
        CacheCommands::List => run_cache_list_command(layout),
        CacheCommands::Prune => run_cache_prune_command(layout),
        CacheCommands::Gc => run_cache_gc_command(layout),
    }
}

fn run_cache_list_command(layout: &PrefixLayout) -> Result<()> {
    let cache_root = layout.artifacts_cache_dir();
    let mut entries = collect_cache_files(&cache_root)?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));

    if entries.is_empty() {
        println!("cache is empty");
        return Ok(());
    }

    println!("path\tbytes");
    for entry in entries {
        let relative = entry
            .path
            .strip_prefix(layout.prefix())
            .unwrap_or(&entry.path)
            .display()
            .to_string();
        println!("{}\t{}", relative, entry.size);
    }
    Ok(())
}

fn run_cache_prune_command(layout: &PrefixLayout) -> Result<()> {
    let cache_root = layout.artifacts_cache_dir();
    let entries = collect_cache_files(&cache_root)?;
    let removed_files = entries.len();
    let removed_bytes = entries.iter().map(|entry| entry.size).sum::<u64>();

    if cache_root.exists() {
        fs::remove_dir_all(&cache_root).with_context(|| {
            format!("failed to remove cache directory: {}", cache_root.display())
        })?;
    }
    fs::create_dir_all(&cache_root).with_context(|| {
        format!(
            "failed to recreate cache directory: {}",
            cache_root.display()
        )
    })?;

    println!("cache prune removed_files={removed_files} removed_bytes={removed_bytes}");
    Ok(())
}

fn run_cache_gc_command(layout: &PrefixLayout) -> Result<()> {
    let cache_root = layout.artifacts_cache_dir();
    let mut entries = collect_cache_files(&cache_root)?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));

    let receipts = read_install_receipts(layout)?;
    let referenced = receipts
        .iter()
        .filter_map(|receipt| receipt.cache_path.as_deref())
        .filter_map(|cache_path| safe_artifact_cache_path(layout, cache_path))
        .collect::<HashSet<_>>();

    let mut removed_files = 0_u64;
    let mut removed_bytes = 0_u64;
    for entry in entries {
        if referenced.contains(&entry.path) {
            continue;
        }
        remove_file_if_exists(&entry.path)
            .with_context(|| format!("failed to remove cache file: {}", entry.path.display()))?;
        removed_files += 1;
        removed_bytes += entry.size;
    }

    let kept_files = referenced.iter().filter(|path| path.exists()).count();
    println!(
        "cache gc removed_files={} removed_bytes={} kept_files={}",
        removed_files, removed_bytes, kept_files
    );
    Ok(())
}

fn safe_artifact_cache_path(layout: &PrefixLayout, cache_path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(cache_path);
    if !path.is_absolute() {
        return None;
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    if !path.starts_with(layout.artifacts_cache_dir()) {
        return None;
    }
    Some(path)
}

fn collect_cache_files(cache_root: &Path) -> Result<Vec<CacheFileEntry>> {
    if !cache_root.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    collect_cache_files_recursive(cache_root, &mut entries)?;
    Ok(entries)
}

fn collect_cache_files_recursive(
    cache_root: &Path,
    entries: &mut Vec<CacheFileEntry>,
) -> Result<()> {
    for item in fs::read_dir(cache_root)
        .with_context(|| format!("failed to read cache directory: {}", cache_root.display()))?
    {
        let item = item?;
        let path = item.path();
        let metadata = item.metadata()?;
        if metadata.is_dir() {
            collect_cache_files_recursive(&path, entries)?;
            continue;
        }
        if metadata.is_file() {
            entries.push(CacheFileEntry {
                path,
                size: metadata.len(),
            });
        }
    }
    Ok(())
}

fn should_render_progress(total_steps: u64) -> bool {
    total_steps > 0
}

fn set_progress(progress: &mut Option<TerminalProgress>, current: u64) {
    if let Some(active_progress) = progress.as_mut() {
        active_progress.set(current);
    }
}

fn print_status_with_progress(
    renderer: TerminalRenderer,
    progress: Option<&TerminalProgress>,
    status: &str,
    message: &str,
) {
    if let Some(active_progress) = progress {
        active_progress.print_status(status, message);
    } else {
        renderer.print_status(status, message);
    }
}

fn print_line_with_progress(progress: Option<&TerminalProgress>, line: &str) {
    if let Some(active_progress) = progress {
        active_progress.print_line(line);
    } else {
        println!("{line}");
    }
}

fn finish_progress(progress: Option<TerminalProgress>) {
    if let Some(active_progress) = progress {
        active_progress.finish_success();
    }
}

struct UpgradeCommandOptions<'a> {
    dry_run: bool,
    explain: bool,
    build_from_source: bool,
    provider_overrides: &'a BTreeMap<String, String>,
    interaction_policy: InstallInteractionPolicy,
}

fn run_upgrade_command(
    layout: &PrefixLayout,
    registry_root: Option<&Path>,
    spec: Option<String>,
    options: UpgradeCommandOptions<'_>,
) -> Result<()> {
    ensure_explain_requires_dry_run("upgrade", options.dry_run, options.explain)?;
    let output_style = current_output_style();
    let renderer = TerminalRenderer::from_style(output_style);
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

    if options.dry_run {
        let mut install_plans = Vec::new();

        match spec.as_deref() {
            Some(single) => {
                let (name, requirement) = parse_spec(single)?;
                let Some(installed_state) = resolve_unambiguous_installed_package(layout, &name)? else {
                    println!("{name} is not installed");
                    return Ok(());
                };
                let installed = receipts.iter().find(|receipt| receipt.name == name);
                let installed_receipt = installed.unwrap_or(&installed_state.receipt);

                let roots = vec![RootInstallRequest {
                    name: installed_receipt.name.clone(),
                    requirement,
                }];
                let resolved = resolve_install_graph(
                    layout,
                    &backend,
                    &roots,
                    installed_receipt.target.as_deref(),
                    options.provider_overrides,
                    options.build_from_source,
                )?;
                enforce_no_downgrades(&receipts, &resolved, "upgrade")?;
                for package in &resolved {
                    validate_install_preflight_for_resolved(layout, package, &receipts)?;
                }
                install_plans.push(build_install_plan_from_resolved(
                    PlanOperation::Upgrade,
                    installed_receipt.target.clone(),
                    &resolved,
                    &receipts,
                    &roots,
                ));
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
                        options.provider_overrides,
                        false,
                        options.build_from_source,
                    )?;
                    enforce_no_downgrades(&receipts, &resolved, "upgrade")?;
                    resolved_dependency_tokens.extend(plan_tokens);
                    install_plans.push(build_install_plan_from_resolved(
                        PlanOperation::Upgrade,
                        plan.target.clone(),
                        &resolved,
                        &receipts,
                        &plan.roots,
                    ));
                    grouped_resolved.push(resolved);
                }

                validate_provider_overrides_used(
                    options.provider_overrides,
                    &resolved_dependency_tokens,
                )?;

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
                }
            }
        }

        let install_plan = merge_install_plans(PlanOperation::Upgrade, None, &install_plans);
        let explainability =
            options.explain.then(|| dependency_policy_explainability_from_install_plan(&install_plan));
        for line in render_install_plan_preview_lines(
            &install_plan,
            TransactionPreviewMode::DryRun,
            explainability.as_ref(),
        ) {
            println!("{line}");
        }
        return Ok(());
    }

    let upgrade_title = match spec.as_deref() {
        Some(single) => format!("Upgrade {single}"),
        None => "Upgrade installed roots".to_string(),
    };
    renderer.print_section(&upgrade_title);

    execute_with_transaction(layout, "upgrade", snapshot_id.as_deref(), |tx| {
        let mut journal_seq = 1_u64;

        match spec.as_deref() {
            Some(single) => {
                let (name, requirement) = parse_spec(single)?;
                let Some(installed_state) = resolve_unambiguous_installed_package(layout, &name)? else {
                    println!("{name} is not installed");
                    return Ok(());
                };
                let installed = receipts.iter().find(|receipt| receipt.name == name);
                let installed_receipt = installed.unwrap_or(&installed_state.receipt);

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
                    options.provider_overrides,
                    options.build_from_source,
                )?;
                let planned_dependency_overrides = build_planned_dependency_overrides(&resolved);
                enforce_no_downgrades(&receipts, &resolved, "upgrade")?;
                let total_packages = resolved.len() as u64;
                let mut completed_packages = 0_u64;
                let mut progress = should_render_progress(total_packages)
                    .then(|| renderer.start_progress("upgrade", total_packages));

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
                let install_plan = build_install_plan_from_resolved(
                    PlanOperation::Upgrade,
                    installed_receipt.target.clone(),
                    &resolved,
                    &receipts,
                    &roots,
                );

                for package in &resolved {
                    set_progress(&mut progress, completed_packages);
                    if let Some(old) = receipts.iter().find(|r| r.name == package.manifest.name) {
                        let old_version = Version::parse(&old.version).with_context(|| {
                            format!(
                                "installed receipt for '{}' has invalid version: {}",
                                old.name, old.version
                            )
                        })?;
                        if package.manifest.version <= old_version {
                            print_status_with_progress(
                                renderer,
                                progress.as_ref(),
                                "step",
                                &format!(
                                    "{} is up-to-date ({})",
                                    package.manifest.name, old.version
                                ),
                            );
                            completed_packages += 1;
                            set_progress(&mut progress, completed_packages);
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

                    let dependencies = build_dependency_receipts(package, &resolved);
                    let mut source_build_journal = SourceBuildJournal {
                        txid: &tx.txid,
                        seq: &mut journal_seq,
                    };
                    let outcome = install_resolved(
                        layout,
                        package,
                        &dependencies,
                        InstallResolvedPlanContext {
                            root_names: &root_names,
                            install_plan: &install_plan,
                            planned_dependency_overrides: &planned_dependency_overrides,
                        },
                        InstallResolvedOptions {
                            snapshot_id: snapshot_id.as_deref(),
                            force_redownload: false,
                            interaction_policy: options.interaction_policy,
                            progress_enabled: current_progress_enabled(output_style),
                        },
                        Some(&mut source_build_journal),
                    )?;
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
                    if let Some(old) = receipts.iter().find(|r| r.name == package.manifest.name) {
                        print_status_with_progress(
                            renderer,
                            progress.as_ref(),
                            "ok",
                            &format!(
                                "upgraded {} from {} to {}",
                                package.manifest.name, old.version, package.manifest.version
                            ),
                        );
                    }
                    print_status_with_progress(
                        renderer,
                        progress.as_ref(),
                        "step",
                        &format!("receipt: {}", outcome.receipt_path.display()),
                    );
                    completed_packages += 1;
                    set_progress(&mut progress, completed_packages);
                }
                finish_progress(progress);
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
                        options.provider_overrides,
                        false,
                        options.build_from_source,
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

                validate_provider_overrides_used(
                    options.provider_overrides,
                    &resolved_dependency_tokens,
                )?;

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
                let total_packages = grouped_resolved
                    .iter()
                    .map(std::vec::Vec::len)
                    .sum::<usize>() as u64;
                let mut completed_packages = 0_u64;
                let mut progress = should_render_progress(total_packages)
                    .then(|| renderer.start_progress("upgrade", total_packages));

                for (resolved, plan) in grouped_resolved.iter().zip(plans.iter()) {
                    let planned_dependency_overrides = build_planned_dependency_overrides(resolved);
                    let install_plan = build_install_plan_from_resolved(
                        PlanOperation::Upgrade,
                        plan.target.clone(),
                        resolved,
                        &receipts,
                        &plan.roots,
                    );

                    for package in resolved {
                        set_progress(&mut progress, completed_packages);
                        if let Some(old) = receipts.iter().find(|r| r.name == package.manifest.name)
                        {
                            let old_version = Version::parse(&old.version).with_context(|| {
                                format!(
                                    "installed receipt for '{}' has invalid version: {}",
                                    old.name, old.version
                                )
                            })?;
                            if package.manifest.version <= old_version {
                                print_status_with_progress(
                                    renderer,
                                    progress.as_ref(),
                                    "step",
                                    &format!(
                                        "{} is up-to-date ({})",
                                        package.manifest.name, old.version
                                    ),
                                );
                                completed_packages += 1;
                                set_progress(&mut progress, completed_packages);
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

                        let dependencies = build_dependency_receipts(package, resolved);
                        let mut source_build_journal = SourceBuildJournal {
                            txid: &tx.txid,
                            seq: &mut journal_seq,
                        };
                        let outcome = install_resolved(
                            layout,
                            package,
                            &dependencies,
                            InstallResolvedPlanContext {
                                root_names: &plan.root_names,
                                install_plan: &install_plan,
                                planned_dependency_overrides: &planned_dependency_overrides,
                            },
                            InstallResolvedOptions {
                                snapshot_id: snapshot_id.as_deref(),
                                force_redownload: false,
                                interaction_policy: options.interaction_policy,
                                progress_enabled: current_progress_enabled(output_style),
                            },
                            Some(&mut source_build_journal),
                        )?;
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
                        if let Some(old) = receipts.iter().find(|r| r.name == package.manifest.name)
                        {
                            print_status_with_progress(
                                renderer,
                                progress.as_ref(),
                                "ok",
                                &format!(
                                    "upgraded {} from {} to {}",
                                    package.manifest.name, old.version, package.manifest.version
                                ),
                            );
                        } else {
                            print_status_with_progress(
                                renderer,
                                progress.as_ref(),
                                "ok",
                                &format!(
                                    "installed dependency {} {}",
                                    package.manifest.name, package.manifest.version
                                ),
                            );
                        }
                        print_status_with_progress(
                            renderer,
                            progress.as_ref(),
                            "step",
                            &format!("receipt: {}", outcome.receipt_path.display()),
                        );
                        completed_packages += 1;
                        set_progress(&mut progress, completed_packages);
                    }
                }
                finish_progress(progress);
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

fn activation_rollback_entry_from_record(
    record: &TransactionJournalRecord,
) -> Result<Option<ActivationRollbackEntry>> {
    if record.step != "integration_activation_rollback" || record.state != "planned" {
        return Ok(None);
    }
    let payload = record
        .path
        .as_deref()
        .ok_or_else(|| anyhow!("integration activation rollback journal entry missing payload"))?;
    Ok(Some(serde_json::from_str(payload).with_context(|| {
        "failed parsing integration activation rollback journal payload"
    })?))
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

fn snapshot_integration_root(snapshot_root: &Path) -> PathBuf {
    snapshot_root.join("integrations")
}

fn snapshot_integration_path(snapshot_root: &Path, integration_storage_rel_path: &str) -> PathBuf {
    snapshot_integration_root(snapshot_root).join(integration_storage_rel_path)
}

fn snapshot_native_sidecar_path(snapshot_root: &Path) -> PathBuf {
    snapshot_root.join("native").join("sidecar.state")
}

fn snapshot_declared_services_sidecar_path(snapshot_root: &Path) -> PathBuf {
    snapshot_root.join("services").join("declared.services")
}

fn snapshot_activation_layout(snapshot_root: &Path) -> PrefixLayout {
    PrefixLayout::new(snapshot_root)
}

fn snapshot_identity_pkgs_root(snapshot_root: &Path) -> PathBuf {
    snapshot_root.join("identity_pkgs")
}

fn snapshot_identity_state_root(snapshot_root: &Path) -> PathBuf {
    snapshot_root.join("identity_state")
}

fn snapshot_identity_pins_root(snapshot_root: &Path) -> PathBuf {
    snapshot_root.join("identity_pins")
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
                integrations: Vec::new(),
                native_sidecar_exists: false,
                declared_services_sidecar_exists: false,
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
        integrations: Vec::new(),
        native_sidecar_exists: false,
        declared_services_sidecar_exists: false,
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
        } else if let Some(integration) = line.strip_prefix("integration=") {
            let mut fields = integration.splitn(3, '\t');
            let (Some(kind), Some(key), Some(rel_path)) =
                (fields.next(), fields.next(), fields.next())
            else {
                return Err(anyhow!("invalid snapshot manifest integration row"));
            };
            manifest.integrations.push(IntegrationProjection {
                kind: kind.to_string(),
                key: key.to_string(),
                rel_path: rel_path.to_string(),
            });
        } else if let Some(value) = line.strip_prefix("native_sidecar_exists=") {
            manifest.native_sidecar_exists = value == "1";
        } else if let Some(value) = line.strip_prefix("declared_services_sidecar_exists=") {
            manifest.declared_services_sidecar_exists = value == "1";
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
    for integration in &manifest.integrations {
        lines.push(format!(
            "integration={}\t{}\t{}",
            integration.kind, integration.key, integration.rel_path
        ));
    }
    lines.push(format!(
        "native_sidecar_exists={}",
        if manifest.native_sidecar_exists {
            "1"
        } else {
            "0"
        }
    ));
    lines.push(format!(
        "declared_services_sidecar_exists={}",
        if manifest.declared_services_sidecar_exists {
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

fn copy_file_if_exists(src: &Path, dst: &Path) -> Result<bool> {
    if !src.exists() {
        return Ok(false);
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    }
    std::fs::copy(src, dst)
        .with_context(|| format!("failed to copy {} to {}", src.display(), dst.display()))?;
    Ok(true)
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
    std::fs::create_dir_all(snapshot_integration_root(&snapshot_root)).with_context(|| {
        format!(
            "failed creating rollback snapshot integration dir: {}",
            snapshot_integration_root(&snapshot_root).display()
        )
    })?;
    std::fs::create_dir_all(snapshot_identity_pkgs_root(&snapshot_root)).with_context(|| {
        format!(
            "failed creating rollback snapshot identity package dir: {}",
            snapshot_identity_pkgs_root(&snapshot_root).display()
        )
    })?;
    std::fs::create_dir_all(snapshot_identity_state_root(&snapshot_root)).with_context(|| {
        format!(
            "failed creating rollback snapshot identity state dir: {}",
            snapshot_identity_state_root(&snapshot_root).display()
        )
    })?;
    std::fs::create_dir_all(snapshot_identity_pins_root(&snapshot_root)).with_context(|| {
        format!(
            "failed creating rollback snapshot identity pins dir: {}",
            snapshot_identity_pins_root(&snapshot_root).display()
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
    let snapshot_services_dir = snapshot_declared_services_sidecar_path(&snapshot_root)
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("failed resolving rollback snapshot services state directory"))?;
    std::fs::create_dir_all(&snapshot_services_dir).with_context(|| {
        format!(
            "failed creating rollback snapshot services state dir: {}",
            snapshot_services_dir.display()
        )
    })?;

    let mut manifest = PackageSnapshotManifest {
        package_exists: false,
        receipt_exists: false,
        bins: Vec::new(),
        completions: Vec::new(),
        gui_assets: Vec::new(),
        integrations: Vec::new(),
        native_sidecar_exists: false,
        declared_services_sidecar_exists: false,
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

    manifest.integrations = read_integration_state(layout, package_name)?;
    for integration in &manifest.integrations {
        let source = layout.integrations_dir().join(&integration.rel_path);
        if source.exists() {
            copy_tree(
                &source,
                &snapshot_integration_path(&snapshot_root, &integration.rel_path),
            )?;
        }
    }

    let activation_records = read_integration_activation_state(layout)?
        .into_iter()
        .filter(|record| record.package == package_name)
        .collect::<Vec<_>>();
    if !activation_records.is_empty() {
        let activation_snapshot = snapshot_activation_layout(&snapshot_root);
        activation_snapshot.ensure_base_dirs()?;
        write_integration_activation_state(&activation_snapshot, &activation_records)?;
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

    let declared_services_sidecar_path = layout.declared_services_state_path(package_name);
    if declared_services_sidecar_path.exists() {
        manifest.declared_services_sidecar_exists = true;
        std::fs::copy(
            &declared_services_sidecar_path,
            snapshot_declared_services_sidecar_path(&snapshot_root),
        )
        .with_context(|| {
            format!(
                "failed copying declared services sidecar snapshot {}",
                snapshot_declared_services_sidecar_path(&snapshot_root).display()
            )
        })?;
    }

    for state in read_all_installed_package_states(layout)?
        .into_iter()
        .filter(|state| state.identity.package == package_name)
    {
        let identity_package_dir = layout.identity_package_dir(&state.identity, &state.version);
        if identity_package_dir.exists() {
            let rel_path = identity_package_dir.strip_prefix(layout.identity_pkgs_dir()).with_context(|| {
                format!(
                    "failed deriving identity package snapshot path for {}",
                    identity_package_dir.display()
                )
            })?;
            copy_tree(
                &identity_package_dir,
                &snapshot_identity_pkgs_root(&snapshot_root).join(rel_path),
            )?;
        }

        let state_key = state.identity.state_key();
        for path in [
            layout.identity_receipt_path(&state.identity),
            layout.installed_identity_state_document_path(&state.identity),
            layout.identity_gui_state_path(&state.identity),
            layout.identity_gui_native_state_path(&state.identity),
            layout.identity_declared_services_state_path(&state.identity),
            layout.identity_integration_state_path(&state.identity),
        ] {
            if let Some(file_name) = path.file_name() {
                copy_file_if_exists(&path, &snapshot_identity_state_root(&snapshot_root).join(file_name))?;
            }
        }
        copy_file_if_exists(
            &layout.identity_pin_path(&state.identity),
            &snapshot_identity_pins_root(&snapshot_root).join(format!("{state_key}.pin")),
        )?;
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
    let existing_identity_states = read_all_installed_package_states(layout)?
        .into_iter()
        .filter(|state| state.identity.package == package_name)
        .collect::<Vec<_>>();
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

    let existing_integrations = read_integration_state(layout, package_name)?;
    for integration in &existing_integrations {
        remove_exposed_integration(layout, integration)?;
    }
    write_integration_state(layout, package_name, &[])?;

    if package_root.exists() {
        std::fs::remove_dir_all(&package_root).with_context(|| {
            format!("failed to remove package path: {}", package_root.display())
        })?;
    }

    for state in &existing_identity_states {
        let identity_package_dir = layout.identity_package_dir(&state.identity, &state.version);
        if identity_package_dir.exists() {
            std::fs::remove_dir_all(&identity_package_dir).with_context(|| {
                format!(
                    "failed to remove identity package path: {}",
                    identity_package_dir.display()
                )
            })?;
        }
        remove_file_if_exists(&layout.identity_receipt_path(&state.identity))?;
        remove_file_if_exists(&layout.installed_identity_state_document_path(&state.identity))?;
        remove_file_if_exists(&layout.identity_gui_state_path(&state.identity))?;
        remove_file_if_exists(&layout.identity_gui_native_state_path(&state.identity))?;
        remove_file_if_exists(&layout.identity_declared_services_state_path(&state.identity))?;
        remove_file_if_exists(&layout.identity_integration_state_path(&state.identity))?;
        remove_file_if_exists(&layout.identity_pin_path(&state.identity))?;
    }

    remove_file_if_exists(&layout.receipt_path(package_name))?;
    remove_file_if_exists(&layout.declared_services_state_path(package_name))?;

    let Some(snapshot_root) = snapshot_root else {
        restore_activation_state_snapshot(layout, package_name, None)?;
        return Ok(());
    };

    let PackageSnapshotManifest {
        package_exists,
        receipt_exists,
        bins,
        completions,
        gui_assets,
        integrations,
        native_sidecar_exists,
        declared_services_sidecar_exists,
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

    for integration in &integrations {
        let dst = layout.integrations_dir().join(&integration.rel_path);
        remove_file_if_exists(&dst)?;
        let src = snapshot_integration_path(snapshot_root, &integration.rel_path);
        if src.exists() {
            copy_tree(&src, &dst).with_context(|| {
                format!(
                    "failed restoring integration '{}' from {}",
                    integration.key,
                    src.display()
                )
            })?;
        }
    }
    write_integration_state(layout, package_name, &integrations)?;
    restore_activation_state_snapshot(layout, package_name, Some(snapshot_root))?;

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

    if declared_services_sidecar_exists {
        let dst = layout.declared_services_state_path(package_name);
        let src = snapshot_declared_services_sidecar_path(snapshot_root);
        remove_file_if_exists(&dst)?;
        if src.exists() {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            std::fs::copy(&src, &dst).with_context(|| {
                format!(
                    "failed restoring declared services sidecar from {}",
                    src.display()
                )
            })?;
        }
    }

    if snapshot_identity_pkgs_root(snapshot_root).exists() {
        copy_tree(
            &snapshot_identity_pkgs_root(snapshot_root),
            &layout.identity_pkgs_dir(),
        )?;
    }
    if snapshot_identity_state_root(snapshot_root).exists() {
        copy_tree(
            &snapshot_identity_state_root(snapshot_root),
            &layout.installed_state_dir(),
        )?;
    }
    if snapshot_identity_pins_root(snapshot_root).exists() {
        copy_tree(
            &snapshot_identity_pins_root(snapshot_root),
            &layout.pins_dir(),
        )?;
    }

    Ok(())
}

fn replay_rollback_journal(layout: &PrefixLayout, txid: &str) -> Result<bool> {
    let records = read_transaction_journal_records(layout, txid)?;
    if records.is_empty() {
        return Ok(false);
    }

    let mut activation_rollbacks = Vec::new();
    for record in records.iter().rev() {
        if let Some(entry) = activation_rollback_entry_from_record(record)? {
            activation_rollbacks.push(entry);
        }
    }

    let mut replayed_activation = false;
    for entry in activation_rollbacks {
        let records = read_integration_activation_state(layout)?;
        let mut fs = real_activation_fs_from_records(current_host_platform(), &records);
        let outcome = replay_activation_rollback_entry_with_fs(&mut fs, &entry);
        if outcome.reason_code != IntegrationReasonCode::Ok {
            return Err(anyhow!(
                "integration activation rollback failed path={} reason={}",
                entry.path,
                outcome.reason_code.as_str()
            ));
        }
        restore_activation_state_after_replayed_rollback(layout, &entry)?;
        replayed_activation = true;
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
    compensating_steps.sort_by_key(|step| std::cmp::Reverse(step.0));

    if compensating_steps.is_empty() {
        let mut backup_steps = backups.into_iter().collect::<Vec<_>>();
        backup_steps.sort_by(|left, right| left.0.cmp(&right.0));
        if backup_steps.is_empty() {
            return Ok(replayed_activation);
        }
        for (package_name, snapshot_root) in backup_steps {
            restore_package_state_snapshot(layout, &package_name, Some(snapshot_root.as_path()))?;
        }
        return Ok(true);
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

fn restore_activation_state_after_replayed_rollback(
    layout: &PrefixLayout,
    entry: &ActivationRollbackEntry,
) -> Result<()> {
    let mut records = read_integration_activation_state(layout)?;
    match entry.operation {
        ActivationRollbackOperation::RemoveCreatedSymlink
        | ActivationRollbackOperation::RemoveCreatedWindowsShim
        | ActivationRollbackOperation::RemoveCreatedServiceMetadata => {
            if let Some(owner) = entry.created_owner.as_ref() {
                records.retain(|record| {
                    !(record.package_state_key == owner.package_state_key
                        && record.package == owner.package
                        && record.integration_key == owner.integration_key)
                });
            } else {
                records.retain(|record| record.host_path.as_deref() != Some(entry.path.as_str()));
            }
        }
        ActivationRollbackOperation::RestoreOwnedSymlink
        | ActivationRollbackOperation::RestoreOwnedWindowsShim
        | ActivationRollbackOperation::RestoreOwnedServiceMetadata => {
            let Some(owner) = entry.previous_owner.as_ref() else {
                write_integration_activation_state(layout, &records)?;
                return Ok(());
            };
            let applied_state = if entry.operation
                == ActivationRollbackOperation::RestoreOwnedServiceMetadata
            {
                IntegrationAppliedState::Stopped
            } else {
                IntegrationAppliedState::Enabled
            };
            if let Some(record) = records.iter_mut().find(|record| {
                record.package_state_key == owner.package_state_key
                    && record.package == owner.package
                    && record.integration_key == owner.integration_key
            }) {
                record.desired_state = if applied_state == IntegrationAppliedState::Running {
                    IntegrationDesiredState::Running
                } else {
                    IntegrationDesiredState::Enabled
                };
                record.applied_state = applied_state;
                record.host_path = Some(entry.path.clone());
                record.reason_code = IntegrationReasonCode::Ok;
            }
        }
    }
    write_integration_activation_state(layout, &records).map(|_| ())
}

fn restore_activation_state_snapshot(
    layout: &PrefixLayout,
    package_name: &str,
    snapshot_root: Option<&Path>,
) -> Result<()> {
    let mut live_records = read_integration_activation_state(layout)?;
    live_records.retain(|record| record.package != package_name);

    if let Some(snapshot_root) = snapshot_root {
        let snapshot_layout = snapshot_activation_layout(snapshot_root);
        live_records.extend(read_integration_activation_state(&snapshot_layout)?);
    }

    write_integration_activation_state(layout, &live_records).map(|_| ())
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
        ensure_transaction_metadata_txid_matches(txid, &metadata)?;
        if matches!(
            metadata.status,
            TransactionStatus::Completed | TransactionStatus::Committed | TransactionStatus::RolledBack
        ) {
            continue;
        }

        match &latest {
            None => latest = Some((metadata.started_at_unix, txid.to_string())),
            Some((best_started_at, best_txid)) => {
                if metadata.started_at_unix > *best_started_at
                    || (metadata.started_at_unix == *best_started_at && txid > best_txid.as_str())
                {
                    latest = Some((metadata.started_at_unix, txid.to_string()));
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
            match read_active_transaction_marker(layout)? {
                ActiveTransactionMarker::Present(active_txid) => active_txid,
                ActiveTransactionMarker::Invalid => {
                    return Err(anyhow!(
                        "transaction state requires repair (reason=active_marker_invalid path={})",
                        layout.transaction_active_path().display()
                    ));
                }
                ActiveTransactionMarker::Absent => {
                    if let Some(candidate_txid) = latest_rollback_candidate_txid(layout)? {
                        candidate_txid
                    } else {
                        println!(
                            "{}",
                            render_status_line(output_style, "step", "no rollback needed")
                        );
                        return Ok(());
                    }
                }
            }
        }
    };

    let metadata = read_transaction_metadata(layout, &target_txid)?
        .ok_or_else(|| anyhow!("transaction metadata missing for rollback txid={target_txid}"))?;
    ensure_transaction_metadata_txid_matches(&target_txid, &metadata)?;
    let active_txid = match read_active_transaction_marker(layout)? {
        ActiveTransactionMarker::Absent => None,
        ActiveTransactionMarker::Invalid => {
            return Err(anyhow!(
                "transaction state requires repair (reason=active_marker_invalid path={})",
                layout.transaction_active_path().display()
            ));
        }
        ActiveTransactionMarker::Present(active_txid) => Some(active_txid),
    };

    if matches!(
        metadata.status,
        TransactionStatus::Planning | TransactionStatus::Applying
    )
        && active_txid.as_deref() == Some(target_txid.as_str())
        && transaction_owner_process_alive(&target_txid)?
    {
        return Err(anyhow!(
            "cannot rollback while transaction is active (status={})",
            metadata.status
        ));
    }

    if matches!(
        metadata.status,
        TransactionStatus::Completed | TransactionStatus::Committed | TransactionStatus::RolledBack
    ) {
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

    set_transaction_status(layout, &target_txid, TransactionStatus::RollingBack)?;
    let replayed = match replay_rollback_journal(layout, &target_txid) {
        Ok(replayed) => replayed,
        Err(err) => {
            let _ = set_transaction_status(layout, &target_txid, TransactionStatus::Failed);
            return Err(err).with_context(|| {
                format!("rollback failed {target_txid}: transaction journal replay required")
            });
        }
    };

    if !replayed && has_completed_mutating_steps {
        let _ = set_transaction_status(layout, &target_txid, TransactionStatus::Failed);
        return Err(anyhow!(
            "rollback failed {target_txid}: transaction journal replay required"
        ));
    }

    set_transaction_status(layout, &target_txid, TransactionStatus::RolledBack)?;

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
    let action = TransactionCoordinator::new(layout).repair_transaction_state()?;
    println!(
        "{}",
        render_status_line(
            output_style,
            "step",
            &format_repair_action_line(&action)
        )
    );

    match &action {
        TransactionRecoveryAction::Clean => {
            println!(
                "{}",
                render_status_line(output_style, "step", "repair: no action needed")
            );
            Ok(())
        }
        TransactionRecoveryAction::CleanupPlanning { txid }
        | TransactionRecoveryAction::FinalizeCommitted { txid }
        | TransactionRecoveryAction::ClearRolledBack { txid } => {
            println!(
                "{}",
                render_status_line(
                    output_style,
                    "ok",
                    &format!("repair: cleared stale marker {txid}")
                )
            );
            Ok(())
        }
        TransactionRecoveryAction::RepairRequired(reason) => {
            Err(anyhow!(format_transaction_preflight_required(layout, reason)))
        }
        TransactionRecoveryAction::Rollback { txid }
        | TransactionRecoveryAction::ResumeRollback { txid }
        | TransactionRecoveryAction::BlockedFailed { txid } => {
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
    }
}

fn format_repair_action_line(action: &TransactionRecoveryAction) -> String {
    format!("repair action={}", transaction_recovery_action_code(action))
}

fn transaction_recovery_action_code(action: &TransactionRecoveryAction) -> &'static str {
    match action {
        TransactionRecoveryAction::Clean => "clean",
        TransactionRecoveryAction::CleanupPlanning { .. } => "cleanup-planning",
        TransactionRecoveryAction::Rollback { .. } => "rollback",
        TransactionRecoveryAction::FinalizeCommitted { .. } => "finalize-committed",
        TransactionRecoveryAction::ResumeRollback { .. } => "resume-rollback",
        TransactionRecoveryAction::ClearRolledBack { .. } => "clear-rolled-back",
        TransactionRecoveryAction::BlockedFailed { .. } => "blocked-failed",
        TransactionRecoveryAction::RepairRequired(reason) => transaction_repair_reason_code(reason),
    }
}

fn transaction_repair_reason_code(reason: &TransactionRepairReason) -> &'static str {
    match reason {
        TransactionRepairReason::ActiveMarkerUnreadable => "active-marker-unreadable",
        TransactionRepairReason::ActiveMarkerInvalid { .. } => "active-marker-invalid",
        TransactionRepairReason::ActiveMarkerWithoutMetadata { .. } => "metadata-missing",
        TransactionRepairReason::MetadataUnreadable { .. } => "metadata-unreadable",
        TransactionRepairReason::MetadataTxidMismatch { .. } => "metadata-txid-mismatch",
        TransactionRepairReason::JournalUnreadable { .. } => "journal-unreadable",
        TransactionRepairReason::ApplyingWithoutActiveMarker { .. } => "applying-without-active-marker",
        TransactionRepairReason::RollbackEvidenceMissing { .. } => "rollback-evidence-missing",
    }
}

fn ensure_transaction_metadata_txid_matches(
    expected_txid: &str,
    metadata: &TransactionMetadata,
) -> Result<()> {
    if metadata.txid != expected_txid {
        return Err(anyhow!(
            "transaction state requires repair (reason=metadata_txid_mismatch expected={} actual={})",
            expected_txid,
            metadata.txid
        ));
    }
    Ok(())
}

#[cfg(test)]
fn run_uninstall_command(layout: &PrefixLayout, name: String) -> Result<()> {
    run_uninstall_command_with_selector(layout, name, None, None, None)
}

fn run_uninstall_command_with_selector(
    layout: &PrefixLayout,
    name: String,
    target: Option<String>,
    profile: Option<String>,
    source: Option<String>,
) -> Result<()> {
    let output_style = current_output_style();
    let renderer = TerminalRenderer::from_style(output_style);
    layout.ensure_base_dirs()?;
    ensure_no_active_transaction_for(layout, "uninstall")?;
    let selector = parse_installed_package_selector(&name, target, profile, source)?;
    let Some(installed_state) = resolve_installed_selector_for_cli(layout, &selector)? else {
        println!("Package not installed: {}", selector.package);
        return Ok(());
    };
    let identity = installed_state.identity.clone();
    let name = installed_state.receipt.name.clone();

    renderer.print_section(&format!("Uninstall {name}"));

    execute_with_transaction(layout, "uninstall", None, |tx| {
        let mut journal_seq = 1_u64;
        for state in read_all_installed_package_states(layout)? {
            let receipt = state.receipt;
            let snapshot_path = capture_package_state_snapshot(layout, &tx.txid, &receipt.name)?;
            append_transaction_journal_entry(
                layout,
                &tx.txid,
                &TransactionJournalEntry {
                    seq: journal_seq,
                    step: format!("backup_package_state:{}", receipt.name),
                    state: "done".to_string(),
                    path: Some(snapshot_path.display().to_string()),
                },
            )?;
            journal_seq += 1;
        }

        let result = if layout.identity_receipt_path(&identity).exists() {
            uninstall_package_identity(layout, &identity)?
        } else {
            uninstall_package(layout, &name)?
        };

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
        let total_steps = if matches!(result.status, UninstallStatus::BlockedByDependents) {
            0
        } else {
            (1 + result.pruned_dependencies.len()) as u64
        };
        let mut progress = should_render_progress(total_steps)
            .then(|| renderer.start_progress("uninstall", total_steps));
        set_progress(&mut progress, 0);
        for line in format_uninstall_messages(&result) {
            print_status_with_progress(renderer, progress.as_ref(), status, &line);
        }
        set_progress(&mut progress, total_steps);
        finish_progress(progress);

        Ok(())
    })?;

    if let Err(err) = sync_completion_assets_best_effort(layout, "uninstall") {
        eprintln!("{err}");
    }

    Ok(())
}

fn run_update_command(store: &RegistrySourceStore, registry: &[String]) -> Result<()> {
    let renderer = TerminalRenderer::current();
    let output_style = renderer.style();
    let results = store.update_sources(registry)?;
    let report = build_update_report(&results);
    let update_output = plan_update_output(&report, output_style);
    let UpdateOutputPlan {
        lines,
        render_progress,
        summary_line,
    } = update_output;
    renderer.print_section("Registry update");
    let total_sources = lines.len() as u64;
    let mut processed_sources = 0_u64;
    let mut progress = render_progress.then(|| renderer.start_progress("update", total_sources));
    for line in lines {
        set_progress(&mut progress, processed_sources);
        print_line_with_progress(progress.as_ref(), &line);
        processed_sources += 1;
        set_progress(&mut progress, processed_sources);
    }
    finish_progress(progress);
    println!("{summary_line}");
    ensure_update_succeeded(report.failed)
}

struct UpdateOutputPlan {
    lines: Vec<String>,
    render_progress: bool,
    summary_line: String,
}

fn plan_update_output(report: &UpdateReport, style: OutputStyle) -> UpdateOutputPlan {
    let lines = format_update_output_lines(report, style);
    let render_progress = should_render_progress(lines.len() as u64);
    let summary_line = format_update_summary_line(report.updated, report.up_to_date, report.failed);
    UpdateOutputPlan {
        lines,
        render_progress,
        summary_line,
    }
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
    let renderer = TerminalRenderer::from_style(output_style);
    layout.ensure_base_dirs()?;
    ensure_no_active_transaction_for(layout, "self-update")?;

    renderer.print_section("Self-update");
    let total_steps = if registry_root.is_none() { 2 } else { 1 };
    let mut completed_steps = 0_u64;

    if registry_root.is_none() {
        renderer.print_status("step", "self-update: refreshing source snapshots");
        let source_state_root = registry_state_root(layout);
        let store = RegistrySourceStore::new(&source_state_root);
        run_update_command(&store, &[])?;
        completed_steps = 1;
    }

    let mut progress = should_render_progress(total_steps)
        .then(|| renderer.start_progress("self-update", total_steps));
    set_progress(&mut progress, completed_steps);

    let args = build_self_update_install_args(registry_root, dry_run, force_redownload, escalation);
    print_status_with_progress(
        renderer,
        progress.as_ref(),
        "step",
        "self-update: installing latest crosspack",
    );
    let result = run_current_exe_command(&args, "self-update install");
    match result {
        Ok(()) => {
            set_progress(&mut progress, total_steps);
            finish_progress(progress);
            Ok(())
        }
        Err(err) => {
            if let Some(active_progress) = progress {
                active_progress.finish_abandon();
            }
            Err(err)
        }
    }
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

fn format_installed_list_lines_for_style(
    style: OutputStyle,
    receipts: &[InstallReceipt],
) -> Vec<String> {
    if receipts.is_empty() {
        return render_empty_state(
            style,
            "No installed packages",
            Some("Run `crosspack install <name>` to install a package."),
        );
    }

    if style == OutputStyle::Plain {
        return receipts
            .iter()
            .map(|receipt| format!("{} {}", receipt.name, receipt.version))
            .collect();
    }

    let mut rows = vec![vec!["name".to_string(), "version".to_string()]];
    for receipt in receipts {
        rows.push(vec![receipt.name.clone(), receipt.version.clone()]);
    }
    render_compact_table(style, &rows)
}

fn format_installed_identity_list_lines(
    style: OutputStyle,
    states: &[InstalledPackageState],
) -> Vec<String> {
    if states.is_empty() {
        return render_empty_state(
            style,
            "No installed packages",
            Some("Run `crosspack install <name>` to install a package."),
        );
    }

    if style == OutputStyle::Plain {
        return states
            .iter()
            .map(|state| {
                format!(
                    "{} {} target={} profile={} source={}",
                    state.receipt.name,
                    state.receipt.version,
                    state.identity.target_label(),
                    state.identity.profile,
                    state.identity.source_namespace_label()
                )
            })
            .collect();
    }

    let mut rows = vec![vec![
        "name".to_string(),
        "version".to_string(),
        "target".to_string(),
        "profile".to_string(),
        "source".to_string(),
    ]];
    for state in states {
        rows.push(vec![
            state.receipt.name.clone(),
            state.receipt.version.clone(),
            state.identity.target_label().to_string(),
            state.identity.profile.clone(),
            state.identity.source_namespace_label().to_string(),
        ]);
    }
    render_compact_table(style, &rows)
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
