use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use crosspack_core::ArchiveType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixLayout {
    prefix: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReceipt {
    pub name: String,
    pub version: String,
    pub target: Option<String>,
    pub artifact_url: Option<String>,
    pub artifact_sha256: Option<String>,
    pub cache_path: Option<String>,
    pub exposed_bins: Vec<String>,
    pub install_status: String,
    pub installed_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UninstallStatus {
    NotInstalled,
    Uninstalled,
    RepairedStaleState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallResult {
    pub name: String,
    pub version: Option<String>,
    pub status: UninstallStatus,
}

impl PrefixLayout {
    pub fn new(prefix: impl Into<PathBuf>) -> Self {
        Self {
            prefix: prefix.into(),
        }
    }

    pub fn prefix(&self) -> &Path {
        &self.prefix
    }

    pub fn pkgs_dir(&self) -> PathBuf {
        self.prefix.join("pkgs")
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.prefix.join("bin")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.prefix.join("state")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.prefix.join("cache")
    }

    pub fn artifacts_cache_dir(&self) -> PathBuf {
        self.cache_dir().join("artifacts")
    }

    pub fn tmp_state_dir(&self) -> PathBuf {
        self.state_dir().join("tmp")
    }

    pub fn installed_state_dir(&self) -> PathBuf {
        self.state_dir().join("installed")
    }

    pub fn pins_dir(&self) -> PathBuf {
        self.state_dir().join("pins")
    }

    pub fn pin_path(&self, name: &str) -> PathBuf {
        self.pins_dir().join(format!("{name}.pin"))
    }

    pub fn package_dir(&self, name: &str, version: &str) -> PathBuf {
        self.pkgs_dir().join(name).join(version)
    }

    pub fn receipt_path(&self, name: &str) -> PathBuf {
        self.installed_state_dir().join(format!("{name}.receipt"))
    }

    pub fn artifact_cache_path(
        &self,
        name: &str,
        version: &str,
        target: &str,
        archive_type: ArchiveType,
    ) -> PathBuf {
        self.artifacts_cache_dir()
            .join(name)
            .join(version)
            .join(target)
            .join(format!("artifact.{}", archive_type.cache_extension()))
    }

    pub fn ensure_base_dirs(&self) -> Result<()> {
        for dir in [
            self.pkgs_dir(),
            self.bin_dir(),
            self.state_dir(),
            self.cache_dir(),
            self.artifacts_cache_dir(),
            self.tmp_state_dir(),
            self.installed_state_dir(),
            self.pins_dir(),
        ] {
            fs::create_dir_all(&dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
        }
        Ok(())
    }
}

pub fn default_user_prefix() -> Result<PathBuf> {
    if cfg!(windows) {
        let app_data = std::env::var("LOCALAPPDATA")
            .context("LOCALAPPDATA is not set; cannot resolve Windows user prefix")?;
        return Ok(PathBuf::from(app_data).join("Crosspack"));
    }

    let home = std::env::var("HOME").context("HOME is not set; cannot resolve user prefix")?;
    Ok(PathBuf::from(home).join(".crosspack"))
}

pub fn install_from_artifact(
    layout: &PrefixLayout,
    name: &str,
    version: &str,
    archive_path: &Path,
    archive_type: ArchiveType,
    strip_components: u32,
    artifact_root: Option<&str>,
) -> Result<PathBuf> {
    let install_tmp = make_tmp_dir(layout, "install")?;
    let raw_dir = install_tmp.join("raw");
    let staged_dir = install_tmp.join("staged");
    fs::create_dir_all(&raw_dir)
        .with_context(|| format!("failed to create {}", raw_dir.display()))?;
    fs::create_dir_all(&staged_dir)
        .with_context(|| format!("failed to create {}", staged_dir.display()))?;

    extract_archive(archive_path, &raw_dir, archive_type)?;

    if let Some(root) = artifact_root {
        let root_path = raw_dir.join(root);
        if !root_path.exists() {
            return Err(anyhow!(
                "artifact_root '{}' was not found after extraction: {}",
                root,
                root_path.display()
            ));
        }
    }

    copy_with_strip(&raw_dir, &staged_dir, strip_components as usize)?;

    let dst = layout.package_dir(name, version);
    if dst.exists() {
        fs::remove_dir_all(&dst)
            .with_context(|| format!("failed to remove existing package dir: {}", dst.display()))?;
    }

    move_dir_or_copy(&staged_dir, &dst)?;

    let _ = fs::remove_dir_all(&install_tmp);
    Ok(dst)
}

pub fn write_install_receipt(layout: &PrefixLayout, receipt: &InstallReceipt) -> Result<PathBuf> {
    let mut payload = String::new();
    payload.push_str(&format!("name={}\n", receipt.name));
    payload.push_str(&format!("version={}\n", receipt.version));
    if let Some(target) = &receipt.target {
        payload.push_str(&format!("target={}\n", target));
    }
    if let Some(url) = &receipt.artifact_url {
        payload.push_str(&format!("artifact_url={}\n", url));
    }
    if let Some(sha256) = &receipt.artifact_sha256 {
        payload.push_str(&format!("artifact_sha256={}\n", sha256));
    }
    if let Some(cache_path) = &receipt.cache_path {
        payload.push_str(&format!("cache_path={}\n", cache_path));
    }
    for exposed_bin in &receipt.exposed_bins {
        payload.push_str(&format!("exposed_bin={}\n", exposed_bin));
    }
    payload.push_str(&format!("install_status={}\n", receipt.install_status));
    payload.push_str(&format!(
        "installed_at_unix={}\n",
        receipt.installed_at_unix
    ));

    let path = layout.receipt_path(&receipt.name);
    fs::write(&path, payload.as_bytes())
        .with_context(|| format!("failed to write install receipt: {}", path.display()))?;
    Ok(path)
}

pub fn read_install_receipts(layout: &PrefixLayout) -> Result<Vec<InstallReceipt>> {
    let dir = layout.installed_state_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut receipts = Vec::new();
    for entry in fs::read_dir(&dir)
        .with_context(|| format!("failed to read install state directory: {}", dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }

        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("receipt") {
            continue;
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read install receipt: {}", path.display()))?;
        let receipt = parse_receipt(&raw)
            .with_context(|| format!("failed to parse install receipt: {}", path.display()))?;
        receipts.push(receipt);
    }

    receipts.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(receipts)
}

pub fn current_unix_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time is before unix epoch")?
        .as_secs())
}

pub fn write_pin(layout: &PrefixLayout, name: &str, requirement: &str) -> Result<PathBuf> {
    let pin_path = layout.pin_path(name);
    if let Some(parent) = pin_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create pin dir: {}", parent.display()))?;
    }

    fs::write(&pin_path, requirement.as_bytes())
        .with_context(|| format!("failed to write pin: {}", pin_path.display()))?;
    Ok(pin_path)
}

pub fn read_pin(layout: &PrefixLayout, name: &str) -> Result<Option<String>> {
    let pin_path = layout.pin_path(name);
    if !pin_path.exists() {
        return Ok(None);
    }

    let value = fs::read_to_string(&pin_path)
        .with_context(|| format!("failed to read pin: {}", pin_path.display()))?;
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(trimmed))
}

pub fn remove_pin(layout: &PrefixLayout, name: &str) -> Result<bool> {
    let pin_path = layout.pin_path(name);
    if !pin_path.exists() {
        return Ok(false);
    }

    fs::remove_file(&pin_path)
        .with_context(|| format!("failed to remove pin: {}", pin_path.display()))?;
    Ok(true)
}

pub fn uninstall_package(layout: &PrefixLayout, name: &str) -> Result<UninstallResult> {
    let receipt_path = layout.receipt_path(name);
    if !receipt_path.exists() {
        return Ok(UninstallResult {
            name: name.to_string(),
            version: None,
            status: UninstallStatus::NotInstalled,
        });
    }

    let raw = fs::read_to_string(&receipt_path)
        .with_context(|| format!("failed to read install receipt: {}", receipt_path.display()))?;
    let receipt = parse_receipt(&raw).with_context(|| {
        format!(
            "failed to parse install receipt: {}",
            receipt_path.display()
        )
    })?;

    let package_dir = layout.package_dir(&receipt.name, &receipt.version);
    let package_existed = package_dir.exists();
    if package_existed {
        fs::remove_dir_all(&package_dir)
            .with_context(|| format!("failed to remove package dir: {}", package_dir.display()))?;
    }

    for exposed_bin in &receipt.exposed_bins {
        remove_exposed_binary(layout, exposed_bin)?;
    }

    fs::remove_file(&receipt_path).with_context(|| {
        format!(
            "failed to remove install receipt: {}",
            receipt_path.display()
        )
    })?;

    Ok(UninstallResult {
        name: receipt.name,
        version: Some(receipt.version),
        status: if package_existed {
            UninstallStatus::Uninstalled
        } else {
            UninstallStatus::RepairedStaleState
        },
    })
}

fn parse_receipt(raw: &str) -> Result<InstallReceipt> {
    let mut name = None;
    let mut version = None;
    let mut target = None;
    let mut artifact_url = None;
    let mut artifact_sha256 = None;
    let mut cache_path = None;
    let mut exposed_bins = Vec::new();
    let mut install_status = None;
    let mut installed_at_unix = None;

    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k {
            "name" => name = Some(v.to_string()),
            "version" => version = Some(v.to_string()),
            "target" => target = Some(v.to_string()),
            "artifact_url" => artifact_url = Some(v.to_string()),
            "artifact_sha256" => artifact_sha256 = Some(v.to_string()),
            "cache_path" => cache_path = Some(v.to_string()),
            "exposed_bin" => exposed_bins.push(v.to_string()),
            "install_status" => install_status = Some(v.to_string()),
            "installed_at_unix" => {
                installed_at_unix = Some(v.parse().context("installed_at_unix must be u64")?)
            }
            _ => {}
        }
    }

    Ok(InstallReceipt {
        name: name.context("missing name")?,
        version: version.context("missing version")?,
        target,
        artifact_url,
        artifact_sha256,
        cache_path,
        exposed_bins,
        install_status: install_status.unwrap_or_else(|| "installed".to_string()),
        installed_at_unix: installed_at_unix.context("missing installed_at_unix")?,
    })
}

pub fn bin_path(layout: &PrefixLayout, binary_name: &str) -> PathBuf {
    let mut file_name = binary_name.to_string();
    if cfg!(windows) {
        file_name.push_str(".cmd");
    }
    layout.bin_dir().join(file_name)
}

pub fn expose_binary(
    layout: &PrefixLayout,
    install_root: &Path,
    binary_name: &str,
    binary_rel_path: &str,
) -> Result<()> {
    let source_rel = validated_relative_binary_path(binary_rel_path)?;
    let source_path = install_root.join(source_rel);
    if !source_path.exists() {
        return Err(anyhow!(
            "declared binary path '{}' was not found in install root: {}",
            binary_rel_path,
            source_path.display()
        ));
    }

    let destination = bin_path(layout, binary_name);
    if destination.exists() {
        fs::remove_file(&destination).with_context(|| {
            format!(
                "failed to replace existing binary entry: {}",
                destination.display()
            )
        })?;
    }

    create_binary_entry(&source_path, &destination)
}

pub fn remove_exposed_binary(layout: &PrefixLayout, binary_name: &str) -> Result<()> {
    let destination = bin_path(layout, binary_name);
    if !destination.exists() {
        return Ok(());
    }

    fs::remove_file(&destination)
        .with_context(|| format!("failed to remove exposed binary: {}", destination.display()))?;
    Ok(())
}

fn validated_relative_binary_path(path: &str) -> Result<&Path> {
    let relative = Path::new(path);
    if relative.is_absolute() {
        return Err(anyhow!("binary path must be relative: {}", path));
    }
    if relative.as_os_str().is_empty() {
        return Err(anyhow!("binary path must not be empty"));
    }
    if relative
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(anyhow!("binary path must not include '..': {}", path));
    }
    Ok(relative)
}

fn create_binary_entry(source_path: &Path, destination: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source_path, destination).with_context(|| {
            format!(
                "failed to create symlink {} -> {}",
                destination.display(),
                source_path.display()
            )
        })
    }

    #[cfg(windows)]
    {
        let shim = format!("@echo off\r\n\"{}\" %*\r\n", source_path.display());
        fs::write(destination, shim.as_bytes())
            .with_context(|| format!("failed to write shim: {}", destination.display()))
    }
}

fn make_tmp_dir(layout: &PrefixLayout, prefix: &str) -> Result<PathBuf> {
    let mut dir = layout.tmp_state_dir();
    dir.push(format!(
        "{}-{}-{}",
        prefix,
        std::process::id(),
        current_unix_timestamp()?
    ));
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed creating tmp dir: {}", dir.display()))?;
    Ok(dir)
}

fn extract_archive(archive_path: &Path, dst: &Path, archive_type: ArchiveType) -> Result<()> {
    match archive_type {
        ArchiveType::Zip => extract_zip(archive_path, dst),
        ArchiveType::TarGz | ArchiveType::TarZst => extract_tar(archive_path, dst),
    }
}

fn extract_tar(archive_path: &Path, dst: &Path) -> Result<()> {
    run_command(
        Command::new("tar")
            .arg("-xf")
            .arg(archive_path)
            .arg("-C")
            .arg(dst),
        "failed to extract tar archive",
    )
}

fn extract_zip(archive_path: &Path, dst: &Path) -> Result<()> {
    if cfg!(windows) {
        let mut command = Command::new("powershell");
        command.arg("-NoProfile").arg("-Command").arg(format!(
            "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
            escape_ps_single_quote(archive_path),
            escape_ps_single_quote(dst)
        ));
        if run_command(
            &mut command,
            "failed to extract zip archive with powershell",
        )
        .is_ok()
        {
            return Ok(());
        }
    }

    let mut unzip_command = Command::new("unzip");
    unzip_command.arg("-q").arg(archive_path).arg("-d").arg(dst);
    if run_command(
        &mut unzip_command,
        "failed to extract zip archive with unzip",
    )
    .is_ok()
    {
        return Ok(());
    }

    run_command(
        Command::new("tar")
            .arg("-xf")
            .arg(archive_path)
            .arg("-C")
            .arg(dst),
        "failed to extract zip archive with tar fallback",
    )
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

fn move_dir_or_copy(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create install parent: {}", parent.display()))?;
    }

    match fs::rename(src, dst) {
        Ok(_) => Ok(()),
        Err(_) => {
            copy_dir_recursive(src, dst)?;
            fs::remove_dir_all(src)
                .with_context(|| format!("failed to cleanup staging dir: {}", src.display()))?;
            Ok(())
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("failed to create {}", dst.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("failed to read {}", src.display()))? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let metadata = fs::symlink_metadata(&src_path)
            .with_context(|| format!("failed to stat {}", src_path.display()))?;
        if metadata.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
            continue;
        }

        #[cfg(unix)]
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(&src_path)
                .with_context(|| format!("failed to read symlink {}", src_path.display()))?;
            std::os::unix::fs::symlink(&target, &dst_path).with_context(|| {
                format!(
                    "failed to create symlink {} -> {}",
                    dst_path.display(),
                    target.display()
                )
            })?;
            continue;
        }

        fs::copy(&src_path, &dst_path).with_context(|| {
            format!(
                "failed to copy {} to {}",
                src_path.display(),
                dst_path.display()
            )
        })?;
    }
    Ok(())
}

fn copy_with_strip(src_root: &Path, dst_root: &Path, strip_components: usize) -> Result<()> {
    let mut copied_any = false;
    copy_with_strip_recursive(
        src_root,
        src_root,
        dst_root,
        strip_components,
        &mut copied_any,
    )?;
    if !copied_any {
        return Err(anyhow!(
            "no files copied during extraction; strip_components={} may be too large",
            strip_components
        ));
    }
    Ok(())
}

fn copy_with_strip_recursive(
    src_root: &Path,
    current: &Path,
    dst_root: &Path,
    strip_components: usize,
    copied_any: &mut bool,
) -> Result<()> {
    for entry in
        fs::read_dir(current).with_context(|| format!("failed to read {}", current.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("failed to stat {}", path.display()))?;

        if metadata.is_dir() {
            copy_with_strip_recursive(src_root, &path, dst_root, strip_components, copied_any)?;
            continue;
        }

        let rel = path
            .strip_prefix(src_root)
            .with_context(|| format!("failed to relativize {}", path.display()))?;
        let stripped = strip_rel_components(rel, strip_components);
        let Some(stripped_rel) = stripped else {
            continue;
        };

        let dst_path = dst_root.join(&stripped_rel);
        if let Some(parent) = dst_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        #[cfg(unix)]
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path)
                .with_context(|| format!("failed to read symlink {}", path.display()))?;
            std::os::unix::fs::symlink(&target, &dst_path).with_context(|| {
                format!(
                    "failed to create symlink {} -> {}",
                    dst_path.display(),
                    target.display()
                )
            })?;
            *copied_any = true;
            continue;
        }

        fs::copy(&path, &dst_path).with_context(|| {
            format!(
                "failed to copy {} to {}",
                path.display(),
                dst_path.display()
            )
        })?;
        *copied_any = true;
    }

    Ok(())
}

fn strip_rel_components(path: &Path, strip_components: usize) -> Option<PathBuf> {
    let components: Vec<_> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(v) => Some(v.to_os_string()),
            _ => None,
        })
        .collect();

    if components.len() <= strip_components {
        return None;
    }

    let mut out = PathBuf::new();
    for component in components.into_iter().skip(strip_components) {
        out.push(component);
    }
    Some(out)
}

fn escape_ps_single_quote(path: &Path) -> String {
    let mut os = OsString::new();
    os.push(path.as_os_str());
    os.to_string_lossy().replace('\'', "''")
}

pub fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        expose_binary, parse_receipt, read_pin, remove_exposed_binary, remove_pin,
        strip_rel_components, uninstall_package, write_install_receipt, write_pin, InstallReceipt,
        PrefixLayout, UninstallStatus,
    };
    use std::fs;
    use std::path::Path;

    #[test]
    fn parse_old_receipt_shape() {
        let raw = "name=fd\nversion=10.2.0\ninstalled_at_unix=123\n";
        let receipt = parse_receipt(raw).expect("must parse");
        assert_eq!(receipt.name, "fd");
        assert_eq!(receipt.version, "10.2.0");
        assert_eq!(receipt.install_status, "installed");
        assert!(receipt.target.is_none());
    }

    #[test]
    fn parse_new_receipt_shape() {
        let raw = "name=fd\nversion=10.2.0\ntarget=x86_64-unknown-linux-gnu\nartifact_url=https://example.test/fd.tgz\nartifact_sha256=abc\ncache_path=/tmp/fd.tgz\nexposed_bin=fd\nexposed_bin=fdfind\ninstall_status=installed\ninstalled_at_unix=123\n";
        let receipt = parse_receipt(raw).expect("must parse");
        assert_eq!(receipt.target.as_deref(), Some("x86_64-unknown-linux-gnu"));
        assert_eq!(receipt.artifact_sha256.as_deref(), Some("abc"));
        assert_eq!(receipt.exposed_bins, vec!["fd", "fdfind"]);
    }

    #[test]
    fn expose_and_remove_binary_round_trip() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let package_dir = layout.package_dir("demo", "1.0.0");
        fs::create_dir_all(&package_dir).expect("must create package dir");
        fs::write(package_dir.join("demo"), b"#!/bin/sh\n").expect("must write binary");

        expose_binary(&layout, &package_dir, "demo", "demo").expect("must expose binary");

        let exposed_path = layout.bin_dir().join("demo");
        assert!(exposed_path.exists());

        remove_exposed_binary(&layout, "demo").expect("must remove binary");
        assert!(!exposed_path.exists());

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn strip_components_behavior() {
        let p = Path::new("top/inner/bin/tool");
        assert_eq!(
            strip_rel_components(p, 1).expect("must exist"),
            Path::new("inner/bin/tool")
        );
        assert!(strip_rel_components(p, 4).is_none());
    }

    #[test]
    fn uninstall_removes_package_dir_and_receipt() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let package_dir = layout.package_dir("demo", "1.0.0");
        fs::create_dir_all(&package_dir).expect("must create package dir");
        fs::write(package_dir.join("demo.txt"), b"hello").expect("must create package file");

        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "demo".to_string(),
                version: "1.0.0".to_string(),
                target: None,
                artifact_url: None,
                artifact_sha256: None,
                cache_path: None,
                exposed_bins: Vec::new(),
                install_status: "installed".to_string(),
                installed_at_unix: 1,
            },
        )
        .expect("must write receipt");

        let result = uninstall_package(&layout, "demo").expect("must uninstall");
        assert_eq!(result.status, UninstallStatus::Uninstalled);
        assert_eq!(result.version.as_deref(), Some("1.0.0"));
        assert!(!layout.receipt_path("demo").exists());
        assert!(!package_dir.exists());

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn uninstall_is_idempotent_when_not_installed() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let result = uninstall_package(&layout, "missing").expect("must be ok");
        assert_eq!(result.status, UninstallStatus::NotInstalled);
        assert_eq!(result.version, None);

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn uninstall_cleans_stale_receipt_when_package_is_missing() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "demo".to_string(),
                version: "1.0.0".to_string(),
                target: None,
                artifact_url: None,
                artifact_sha256: None,
                cache_path: None,
                exposed_bins: Vec::new(),
                install_status: "installed".to_string(),
                installed_at_unix: 1,
            },
        )
        .expect("must write receipt");

        let result = uninstall_package(&layout, "demo").expect("must uninstall stale state");
        assert_eq!(result.status, UninstallStatus::RepairedStaleState);
        assert!(!layout.receipt_path("demo").exists());

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn uninstall_parse_failure_preserves_files() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let package_dir = layout.package_dir("demo", "1.0.0");
        fs::create_dir_all(&package_dir).expect("must create package dir");
        let receipt_path = layout.receipt_path("demo");
        fs::write(&receipt_path, b"name=demo\nversion=1.0.0\n").expect("must write malformed");

        let err = uninstall_package(&layout, "demo").expect_err("must fail on malformed receipt");
        assert!(err.to_string().contains("failed to parse install receipt"));
        assert!(receipt_path.exists());
        assert!(package_dir.exists());

        let _ = fs::remove_dir_all(layout.prefix());
    }

    fn test_layout() -> PrefixLayout {
        let mut path = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        path.push(format!(
            "crosspack-installer-tests-{}-{}",
            std::process::id(),
            nanos
        ));
        PrefixLayout::new(path)
    }

    #[test]
    fn pin_round_trip() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        write_pin(&layout, "ripgrep", "^14").expect("must write pin");
        let pin = read_pin(&layout, "ripgrep").expect("must read pin");
        assert_eq!(pin.as_deref(), Some("^14"));

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn pin_overwrite_replaces_old_value() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        write_pin(&layout, "ripgrep", "^13").expect("must write pin");
        write_pin(&layout, "ripgrep", "^14").expect("must overwrite pin");
        let pin = read_pin(&layout, "ripgrep").expect("must read pin");
        assert_eq!(pin.as_deref(), Some("^14"));

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn remove_pin_returns_false_when_missing() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let removed = remove_pin(&layout, "missing").expect("must handle missing");
        assert!(!removed);

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn remove_pin_returns_true_when_existing() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        write_pin(&layout, "ripgrep", "^14").expect("must write pin");
        let removed = remove_pin(&layout, "ripgrep").expect("must remove existing");
        assert!(removed);
        let pin = read_pin(&layout, "ripgrep").expect("must read pin");
        assert!(pin.is_none());

        let _ = fs::remove_dir_all(layout.prefix());
    }
}
