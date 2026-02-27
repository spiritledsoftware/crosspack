use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(unix)]
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use crosspack_core::{
    ArchiveType, Artifact, ArtifactCompletionShell, ArtifactGuiApp, PackageManifest,
};
use crosspack_installer::{
    append_transaction_journal_entry, bin_path, clear_active_transaction, current_unix_timestamp,
    default_user_prefix, expose_binary, expose_completion, expose_gui_app, exposed_completion_path,
    gui_asset_path, install_from_artifact, projected_exposed_completion_path, projected_gui_assets,
    read_active_transaction, read_all_gui_exposure_states, read_all_pins, read_gui_exposure_state,
    read_gui_native_state, read_install_receipts, read_transaction_metadata,
    register_native_gui_app_best_effort, remove_exposed_binary, remove_exposed_completion,
    remove_exposed_gui_asset, remove_file_if_exists, remove_native_gui_registration_best_effort,
    run_package_native_uninstall_actions, set_active_transaction,
    uninstall_blocked_by_roots_with_dependency_overrides_and_ignored_roots, uninstall_package,
    uninstall_package_with_dependency_overrides_and_ignored_roots, update_transaction_status,
    write_gui_exposure_state, write_gui_native_state, write_install_receipt, write_pin,
    write_transaction_metadata, ArtifactInstallOptions, GuiExposureAsset,
    GuiNativeRegistrationRecord, InstallInteractionPolicy, InstallMode, InstallReason,
    InstallReceipt, PrefixLayout, TransactionJournalEntry, TransactionMetadata, UninstallResult,
    UninstallStatus,
};
use crosspack_registry::{
    ConfiguredRegistryIndex, RegistryIndex, RegistrySourceKind, RegistrySourceRecord,
    RegistrySourceSnapshotState, RegistrySourceStore, RegistrySourceWithSnapshotState,
    SourceUpdateResult, SourceUpdateStatus,
};
use crosspack_resolver::{resolve_dependency_graph, RootRequirement};
use crosspack_security::verify_sha256_file;
use semver::{Version, VersionReq};
use serde_json::Value;

#[derive(Parser, Debug)]
#[command(name = "crosspack")]
#[command(version)]
#[command(about = "Native cross-platform package manager", long_about = None)]
struct Cli {
    #[arg(long)]
    registry_root: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

const NO_ROOT_PACKAGES_TO_UPGRADE: &str = "No root packages installed";
const METADATA_CONFIG_GUIDANCE: &str =
    "no configured registry snapshots available; bootstrap trusted source `core` with `crosspack registry add core https://github.com/spiritledsoftware/crosspack-registry.git --kind git --priority 100 --fingerprint <64-hex>` then run `crosspack update` (see https://github.com/spiritledsoftware/crosspack/blob/main/docs/registry-bootstrap-runbook.md and https://github.com/spiritledsoftware/crosspack/blob/main/docs/trust/core-registry-fingerprint.txt)";
const SNAPSHOT_ID_MISMATCH_ERROR_CODE: &str = "snapshot-id-mismatch";
const SEARCH_METADATA_GUIDANCE: &str =
    "search metadata unavailable; run `crosspack update` to refresh local snapshots and `crosspack registry list` to inspect source status";

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum OutputStyle {
    Plain,
    Rich,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum InstallProgressMode {
    Disabled,
    Ascii,
    Unicode,
}

const ASCII_PROGRESS_FRAMES: [&str; 4] = ["-", "\\", "|", "/"];
const UNICODE_PROGRESS_FRAMES: [&str; 10] = [
    "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}", "\u{2827}",
    "\u{2807}", "\u{280f}",
];

fn locale_looks_utf8(locale: &str) -> bool {
    let lower = locale.to_ascii_lowercase();
    lower.contains("utf-8") || lower.contains("utf8")
}

fn resolve_install_progress_mode(style: OutputStyle, locale: Option<&str>) -> InstallProgressMode {
    if style == OutputStyle::Plain {
        return InstallProgressMode::Disabled;
    }

    match locale {
        Some(value) if locale_looks_utf8(value) => InstallProgressMode::Unicode,
        _ => InstallProgressMode::Ascii,
    }
}

fn current_install_progress_mode(style: OutputStyle) -> InstallProgressMode {
    let locale = std::env::var("LC_ALL")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("LC_CTYPE")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .or_else(|| std::env::var("LANG").ok().filter(|value| !value.is_empty()));
    resolve_install_progress_mode(style, locale.as_deref())
}

fn install_progress_frames(mode: InstallProgressMode) -> &'static [&'static str] {
    match mode {
        InstallProgressMode::Disabled => &ASCII_PROGRESS_FRAMES,
        InstallProgressMode::Ascii => &ASCII_PROGRESS_FRAMES,
        InstallProgressMode::Unicode => &UNICODE_PROGRESS_FRAMES,
    }
}

struct InstallProgressLineState<'a> {
    phase: &'a str,
    step: usize,
    total_steps: usize,
    download_progress: Option<(u64, Option<u64>)>,
}

fn format_install_progress_line(
    mode: InstallProgressMode,
    frame_index: usize,
    action: &str,
    package: &str,
    state: InstallProgressLineState<'_>,
) -> String {
    let frame = install_progress_frames(mode)[frame_index % install_progress_frames(mode).len()];
    let step_progress = if state.total_steps == 0 {
        0.0
    } else {
        (state.step.min(state.total_steps) as f32) / (state.total_steps as f32)
    };
    let bar_width = 20_usize;
    let render_progress_bar = |progress: f32| {
        let filled = (progress * (bar_width as f32)).round() as usize;
        let filled = filled.min(bar_width);
        format!(
            "{}{}",
            "=".repeat(filled),
            "-".repeat(bar_width.saturating_sub(filled))
        )
    };
    let bar = if state.phase == "download" {
        match state.download_progress {
            Some((downloaded, Some(total_bytes))) if total_bytes > 0 => {
                let download_progress =
                    (downloaded as f64 / total_bytes as f64).clamp(0.0, 1.0) as f32;
                render_progress_bar(download_progress)
            }
            Some((_downloaded, None)) => {
                let mut buffer = vec!['-'; bar_width];
                let fill_width = (bar_width / 4).max(1);
                let start = frame_index % bar_width;
                for offset in 0..fill_width {
                    buffer[(start + offset) % bar_width] = '=';
                }
                buffer.into_iter().collect::<String>()
            }
            _ => render_progress_bar(step_progress),
        }
    } else {
        render_progress_bar(step_progress)
    };
    let transfer = state
        .download_progress
        .map(|(downloaded, total)| match total {
            Some(total_bytes) if total_bytes > 0 => format!(
                " {}B/{}B ({:.0}%)",
                downloaded,
                total_bytes,
                ((downloaded as f64) / (total_bytes as f64) * 100.0).clamp(0.0, 100.0)
            ),
            Some(total_bytes) => format!(" {}B/{}B", downloaded, total_bytes),
            None => format!(" {}B", downloaded),
        })
        .unwrap_or_default();

    format!(
        "{frame} {action} {package:<12} [{bar}] {}/{} {phase}",
        state.step.min(state.total_steps),
        state.total_steps,
        phase = state.phase,
    ) + &transfer
}

struct InstallProgressRenderer {
    mode: InstallProgressMode,
    action: String,
    package: String,
    frame_index: usize,
    total_steps: usize,
    active: bool,
    completed: bool,
    last_phase: Option<String>,
    last_step: Option<usize>,
    last_redraw_at: Option<Instant>,
}

const DOWNLOAD_PROGRESS_REDRAW_INTERVAL: Duration = Duration::from_millis(80);

fn install_progress_renderer_finish_sequence(completed: bool) -> &'static str {
    if completed {
        "\n"
    } else {
        "\r\x1b[2K"
    }
}

fn should_render_install_progress_update(
    previous_phase: Option<&str>,
    previous_step: Option<usize>,
    phase: &str,
    step: usize,
    elapsed_since_last_redraw: Option<Duration>,
) -> bool {
    if previous_phase != Some(phase) || previous_step != Some(step) {
        return true;
    }
    if phase != "download" {
        return true;
    }

    elapsed_since_last_redraw
        .map(|elapsed| elapsed >= DOWNLOAD_PROGRESS_REDRAW_INTERVAL)
        .unwrap_or(true)
}

impl InstallProgressRenderer {
    fn new(mode: InstallProgressMode, action: &str, package: &str, total_steps: usize) -> Self {
        Self {
            mode,
            action: action.to_string(),
            package: package.to_string(),
            frame_index: 0,
            total_steps,
            active: false,
            completed: false,
            last_phase: None,
            last_step: None,
            last_redraw_at: None,
        }
    }

    fn update(&mut self, phase: &str, step: usize, download_progress: Option<(u64, Option<u64>)>) {
        if self.mode == InstallProgressMode::Disabled {
            return;
        }

        let now = Instant::now();
        let elapsed_since_last_redraw = self
            .last_redraw_at
            .map(|last_redraw_at| now.saturating_duration_since(last_redraw_at));
        if !should_render_install_progress_update(
            self.last_phase.as_deref(),
            self.last_step,
            phase,
            step,
            elapsed_since_last_redraw,
        ) {
            return;
        }

        let line = format_install_progress_line(
            self.mode,
            self.frame_index,
            &self.action,
            &self.package,
            InstallProgressLineState {
                phase,
                step,
                total_steps: self.total_steps,
                download_progress,
            },
        );
        let mut stdout = std::io::stdout();
        if write!(stdout, "\r\x1b[2K{line}").is_ok() {
            let _ = stdout.flush();
        }
        self.frame_index = self.frame_index.wrapping_add(1);
        self.active = true;
        self.completed = self.total_steps > 0 && step >= self.total_steps;
        self.last_phase = Some(phase.to_string());
        self.last_step = Some(step);
        self.last_redraw_at = Some(now);
    }

    fn finish(&mut self) {
        if self.mode == InstallProgressMode::Disabled || !self.active {
            return;
        }

        let mut stdout = std::io::stdout();
        if write!(
            stdout,
            "{}",
            install_progress_renderer_finish_sequence(self.completed)
        )
        .is_ok()
        {
            let _ = stdout.flush();
        }
        self.active = false;
        self.completed = false;
    }
}

impl Drop for InstallProgressRenderer {
    fn drop(&mut self) {
        self.finish();
    }
}

#[derive(Args, Copy, Clone, Debug, Default, Eq, PartialEq)]
struct EscalationArgs {
    #[arg(long)]
    non_interactive: bool,
    #[arg(long, conflicts_with = "no_escalation")]
    allow_escalation: bool,
    #[arg(long, conflicts_with = "allow_escalation")]
    no_escalation: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct EscalationPolicy {
    allow_prompt_escalation: bool,
    allow_non_prompt_escalation: bool,
}

fn resolve_escalation_policy(args: EscalationArgs) -> EscalationPolicy {
    if args.no_escalation {
        return EscalationPolicy {
            allow_prompt_escalation: false,
            allow_non_prompt_escalation: false,
        };
    }

    if args.non_interactive {
        return EscalationPolicy {
            allow_prompt_escalation: false,
            allow_non_prompt_escalation: args.allow_escalation,
        };
    }

    EscalationPolicy {
        allow_prompt_escalation: true,
        allow_non_prompt_escalation: true,
    }
}

fn install_interaction_policy(escalation_policy: EscalationPolicy) -> InstallInteractionPolicy {
    InstallInteractionPolicy {
        allow_prompt_escalation: escalation_policy.allow_prompt_escalation,
        allow_non_prompt_escalation: escalation_policy.allow_non_prompt_escalation,
    }
}

fn install_mode_for_archive_type(archive_type: ArchiveType) -> InstallMode {
    match archive_type {
        ArchiveType::Zip
        | ArchiveType::TarGz
        | ArchiveType::TarZst
        | ArchiveType::Bin
        | ArchiveType::Dmg
        | ArchiveType::AppImage => InstallMode::Managed,
        ArchiveType::Msi
        | ArchiveType::Exe
        | ArchiveType::Pkg
        | ArchiveType::Msix
        | ArchiveType::Appx => InstallMode::Native,
    }
}

fn build_artifact_install_options<'a>(
    resolved: &'a ResolvedInstall,
    interaction_policy: InstallInteractionPolicy,
) -> ArtifactInstallOptions<'a> {
    ArtifactInstallOptions {
        strip_components: resolved.artifact.strip_components.unwrap_or(0),
        artifact_root: resolved.artifact.artifact_root.as_deref(),
        install_mode: install_mode_for_archive_type(resolved.archive_type),
        interaction_policy,
    }
}

fn resolve_output_style(stdout_is_tty: bool, _stderr_is_tty: bool) -> OutputStyle {
    if stdout_is_tty {
        OutputStyle::Rich
    } else {
        OutputStyle::Plain
    }
}

fn render_status_line(style: OutputStyle, status: &str, message: &str) -> String {
    match style {
        OutputStyle::Plain => message.to_string(),
        OutputStyle::Rich => {
            let badge = match status {
                "ok" => "[OK]",
                "warn" => "[WARN]",
                "error" => "[ERR]",
                "step" => "[..]",
                _ => "[*]",
            };
            format!("{badge} {message}")
        }
    }
}

fn render_update_line(style: OutputStyle, line: &str) -> String {
    if line.contains(": failed") {
        return render_status_line(style, "error", line);
    }
    if line.contains(": updated") {
        return render_status_line(style, "ok", line);
    }
    if line.contains(": up-to-date") {
        return render_status_line(style, "step", line);
    }
    render_status_line(style, "step", line)
}

fn format_update_output_lines(report: &UpdateReport, style: OutputStyle) -> Vec<String> {
    report
        .lines
        .iter()
        .map(|line| render_update_line(style, line))
        .collect()
}

fn current_output_style() -> OutputStyle {
    resolve_output_style(
        std::io::stdout().is_terminal(),
        std::io::stderr().is_terminal(),
    )
}

#[derive(Subcommand, Debug)]
enum Commands {
    Search {
        query: String,
    },
    Info {
        name: String,
    },
    Install {
        spec: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        force_redownload: bool,
        #[arg(long = "provider", value_name = "capability=package")]
        provider: Vec<String>,
        #[command(flatten)]
        escalation: EscalationArgs,
    },
    Upgrade {
        spec: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long = "provider", value_name = "capability=package")]
        provider: Vec<String>,
        #[command(flatten)]
        escalation: EscalationArgs,
    },
    Rollback {
        txid: Option<String>,
        #[command(flatten)]
        escalation: EscalationArgs,
    },
    Repair {
        #[command(flatten)]
        escalation: EscalationArgs,
    },
    Uninstall {
        name: String,
        #[command(flatten)]
        escalation: EscalationArgs,
    },
    List,
    Pin {
        spec: String,
    },
    Registry {
        #[command(subcommand)]
        command: RegistryCommands,
    },
    Update {
        #[arg(long = "registry")]
        registry: Vec<String>,
    },
    SelfUpdate {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        force_redownload: bool,
        #[command(flatten)]
        escalation: EscalationArgs,
    },
    Doctor,
    Version,
    Completions {
        shell: CliCompletionShell,
    },
    InitShell {
        #[arg(long)]
        shell: Option<CliCompletionShell>,
    },
}

#[derive(Subcommand, Debug)]
enum RegistryCommands {
    Add {
        name: String,
        location: String,
        #[arg(long)]
        kind: CliRegistryKind,
        #[arg(long)]
        priority: u32,
        #[arg(long)]
        fingerprint: String,
    },
    List,
    Remove {
        name: String,
        #[arg(long)]
        purge_cache: bool,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum CliRegistryKind {
    Git,
    Filesystem,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum CliCompletionShell {
    Bash,
    Zsh,
    Fish,
    Powershell,
}

impl From<CliRegistryKind> for RegistrySourceKind {
    fn from(value: CliRegistryKind) -> Self {
        match value {
            CliRegistryKind::Git => RegistrySourceKind::Git,
            CliRegistryKind::Filesystem => RegistrySourceKind::Filesystem,
        }
    }
}

impl From<CliCompletionShell> for Shell {
    fn from(value: CliCompletionShell) -> Self {
        match value {
            CliCompletionShell::Bash => Shell::Bash,
            CliCompletionShell::Zsh => Shell::Zsh,
            CliCompletionShell::Fish => Shell::Fish,
            CliCompletionShell::Powershell => Shell::PowerShell,
        }
    }
}

impl CliCompletionShell {
    fn completion_filename(self) -> &'static str {
        match self {
            Self::Bash => "crosspack.bash",
            Self::Zsh => "crosspack.zsh",
            Self::Fish => "crosspack.fish",
            Self::Powershell => "crosspack.ps1",
        }
    }

    fn package_completion_shell(self) -> ArtifactCompletionShell {
        match self {
            Self::Bash => ArtifactCompletionShell::Bash,
            Self::Zsh => ArtifactCompletionShell::Zsh,
            Self::Fish => ArtifactCompletionShell::Fish,
            Self::Powershell => ArtifactCompletionShell::Powershell,
        }
    }
}

fn main() -> Result<()> {
    run_cli(Cli::parse())
}

include!("completion.rs");

include!("dispatch.rs");

include!("metadata.rs");

include!("command_flows.rs");

include!("core_flows.rs");

include!("tests.rs");
