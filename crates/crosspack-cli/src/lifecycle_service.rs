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
struct ListCommandRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListCommandOutcome {
    receipts: Vec<InstallReceipt>,
}

fn build_list_command_outcome(receipts: Vec<InstallReceipt>) -> ListCommandOutcome {
    ListCommandOutcome { receipts }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LifecycleCommandOutcome {
    Lines(Vec<String>),
}
