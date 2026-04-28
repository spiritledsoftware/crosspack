#[derive(Debug, Clone, PartialEq, Eq)]
struct InstallCommandRequest {
    spec: String,
    target: Option<String>,
    dry_run: bool,
    force_redownload: bool,
    build_from_source: bool,
    explain: bool,
    provider_overrides: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpgradeCommandRequest {
    spec: Option<String>,
    target: Option<String>,
    dry_run: bool,
    force_redownload: bool,
    build_from_source: bool,
    explain: bool,
    provider_overrides: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UninstallCommandRequest {
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LifecycleCommandOutcome {
    Lines(Vec<String>),
}
