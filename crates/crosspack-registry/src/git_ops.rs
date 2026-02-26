use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

pub(crate) fn base_git_command() -> Command {
    let mut command = Command::new("git");
    command
        .arg("-c")
        .arg("core.autocrlf=false")
        .arg("-c")
        .arg("core.eol=lf");
    if cfg!(windows) {
        command.arg("-c").arg("core.longpaths=true");
    }
    command
}

pub(crate) fn run_git_clone(location: &str, destination: &Path, source_name: &str) -> Result<()> {
    let output = base_git_command()
        .arg("clone")
        .arg("--")
        .arg(location)
        .arg(destination)
        .output()
        .with_context(|| {
            format!(
                "source-sync-failed: source '{}' failed launching git clone",
                source_name
            )
        })?;
    if !output.status.success() {
        anyhow::bail!(
            "source-sync-failed: source '{}' git clone failed: {}",
            source_name,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

pub(crate) fn run_git_command(repo_root: &Path, args: &[&str], source_name: &str) -> Result<()> {
    let output = base_git_command()
        .args(args)
        .current_dir(repo_root)
        .output()
        .with_context(|| {
            format!(
                "source-sync-failed: source '{}' failed launching git {}",
                source_name,
                args.join(" ")
            )
        })?;
    if !output.status.success() {
        anyhow::bail!(
            "source-sync-failed: source '{}' git {} failed: {}",
            source_name,
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

pub(crate) fn git_head_snapshot_id(repo_root: &Path, source_name: &str) -> Result<String> {
    let output = base_git_command()
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(repo_root)
        .output()
        .with_context(|| {
            format!(
                "source-sync-failed: source '{}' failed launching git rev-parse",
                source_name
            )
        })?;
    if !output.status.success() {
        anyhow::bail!(
            "source-sync-failed: source '{}' git rev-parse failed: {}",
            source_name,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let snapshot_id = String::from_utf8(output.stdout)
        .context("source-sync-failed: git rev-parse produced non-UTF-8 output")?
        .trim()
        .to_string();
    derive_snapshot_id_from_full_git_sha(&snapshot_id).with_context(|| {
        format!(
            "source-sync-failed: source '{}' git rev-parse returned invalid HEAD sha",
            source_name
        )
    })
}

pub(crate) fn derive_snapshot_id_from_full_git_sha(full_sha: &str) -> Result<String> {
    let normalized = full_sha.trim();
    if normalized.len() < 16 {
        anyhow::bail!("git HEAD sha too short for snapshot id: '{normalized}'");
    }
    if !normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        anyhow::bail!("git HEAD sha contains non-hex characters: '{normalized}'");
    }

    Ok(format!(
        "git:{}",
        normalized.chars().take(16).collect::<String>()
    ))
}
