use anyhow::{anyhow, Context, Result};
use crosspack_core::ArchiveType;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crate::transactions::current_unix_timestamp;
use crate::{ArtifactInstallOptions, InstallInteractionPolicy, InstallMode, PrefixLayout};

pub fn install_from_artifact(
    layout: &PrefixLayout,
    name: &str,
    version: &str,
    archive_path: &Path,
    archive_type: ArchiveType,
    options: ArtifactInstallOptions<'_>,
) -> Result<PathBuf> {
    let install_tmp = make_tmp_dir(layout, "install")?;
    let raw_dir = install_tmp.join("raw");
    let staged_dir = install_tmp.join("staged");
    fs::create_dir_all(&raw_dir)
        .with_context(|| format!("failed to create {}", raw_dir.display()))?;
    fs::create_dir_all(&staged_dir)
        .with_context(|| format!("failed to create {}", staged_dir.display()))?;

    stage_artifact_payload(
        archive_path,
        &raw_dir,
        archive_type,
        options.strip_components,
        options.artifact_root,
        options.install_mode,
        options.interaction_policy,
    )?;

    if let Some(root) = options.artifact_root {
        let root_path = raw_dir.join(root);
        if !root_path.exists() {
            return Err(anyhow!(
                "artifact_root '{}' was not found after extraction: {}",
                root,
                root_path.display()
            ));
        }
    }

    copy_with_strip(&raw_dir, &staged_dir, options.strip_components as usize)?;

    let dst = layout.package_dir(name, version);
    if dst.exists() {
        fs::remove_dir_all(&dst)
            .with_context(|| format!("failed to remove existing package dir: {}", dst.display()))?;
    }

    move_dir_or_copy(&staged_dir, &dst)?;

    let _ = fs::remove_dir_all(&install_tmp);
    Ok(dst)
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

fn stage_artifact_payload(
    artifact_path: &Path,
    raw_dir: &Path,
    artifact_type: ArchiveType,
    strip_components: u32,
    artifact_root: Option<&str>,
    install_mode: InstallMode,
    interaction_policy: InstallInteractionPolicy,
) -> Result<()> {
    if install_mode == InstallMode::Native
        && is_native_default_archive_type(artifact_type)
        && !interaction_policy.allow_prompt_escalation
        && !interaction_policy.allow_non_prompt_escalation
    {
        return Err(anyhow!(
            "native installer mode requires escalation but policy forbids it for archive type '{}'",
            artifact_type.as_str()
        ));
    }

    match artifact_type {
        ArchiveType::Zip => extract_zip(artifact_path, raw_dir),
        ArchiveType::TarGz | ArchiveType::TarZst => extract_tar(artifact_path, raw_dir),
        ArchiveType::Bin => {
            stage_bin_payload(artifact_path, raw_dir, strip_components, artifact_root)
        }
        ArchiveType::AppImage => {
            stage_appimage_payload(artifact_path, raw_dir, strip_components, artifact_root)
        }
        ArchiveType::Msi => stage_msi_payload(artifact_path, raw_dir),
        ArchiveType::Dmg => stage_dmg_payload(artifact_path, raw_dir),
        ArchiveType::Exe => stage_exe_payload(artifact_path, raw_dir),
        ArchiveType::Pkg => stage_pkg_payload(artifact_path, raw_dir),
        ArchiveType::Msix => stage_msix_payload(artifact_path, raw_dir),
        ArchiveType::Appx => stage_appx_payload(artifact_path, raw_dir),
    }
}

fn is_native_default_archive_type(artifact_type: ArchiveType) -> bool {
    matches!(
        artifact_type,
        ArchiveType::Msi
            | ArchiveType::Exe
            | ArchiveType::Pkg
            | ArchiveType::Msix
            | ArchiveType::Appx
    )
}

pub(crate) fn stage_appimage_payload(
    artifact_path: &Path,
    raw_dir: &Path,
    strip_components: u32,
    artifact_root: Option<&str>,
) -> Result<()> {
    if !cfg!(target_os = "linux") {
        return Err(anyhow!(
            "AppImage artifacts are supported only on Linux hosts"
        ));
    }

    if strip_components != 0 {
        return Err(anyhow!("strip_components must be 0 for AppImage artifacts"));
    }
    if artifact_root.is_some_and(|value| !value.trim().is_empty()) {
        return Err(anyhow!(
            "artifact_root is not supported for AppImage artifacts"
        ));
    }

    fs::create_dir_all(raw_dir)
        .with_context(|| format!("failed to create {}", raw_dir.display()))?;
    let staged = raw_dir.join("artifact.appimage");
    fs::copy(artifact_path, &staged).with_context(|| {
        format!(
            "failed to stage AppImage payload from {} to {}",
            artifact_path.display(),
            staged.display()
        )
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&staged)
            .with_context(|| format!("failed to stat {}", staged.display()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&staged, permissions)
            .with_context(|| format!("failed to set executable mode on {}", staged.display()))?;
    }

    Ok(())
}

pub(crate) fn stage_bin_payload(
    artifact_path: &Path,
    raw_dir: &Path,
    strip_components: u32,
    artifact_root: Option<&str>,
) -> Result<()> {
    if strip_components != 0 {
        return Err(anyhow!("strip_components must be 0 for bin artifacts"));
    }
    if artifact_root.is_some_and(|value| !value.trim().is_empty()) {
        return Err(anyhow!("artifact_root is not supported for bin artifacts"));
    }

    fs::create_dir_all(raw_dir)
        .with_context(|| format!("failed to create {}", raw_dir.display()))?;
    let file_name = artifact_path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| anyhow!("failed to derive bin artifact file name"))?;
    let staged = raw_dir.join(file_name);
    fs::copy(artifact_path, &staged).with_context(|| {
        format!(
            "failed to stage bin payload from {} to {}",
            artifact_path.display(),
            staged.display()
        )
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&staged)
            .with_context(|| format!("failed to stat {}", staged.display()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&staged, permissions)
            .with_context(|| format!("failed to set executable mode on {}", staged.display()))?;
    }

    Ok(())
}

fn stage_msi_payload(_artifact_path: &Path, _raw_dir: &Path) -> Result<()> {
    if !cfg!(windows) {
        return Err(anyhow!("MSI artifacts are supported only on Windows hosts"));
    }
    let mut command = build_msi_admin_extract_command(_artifact_path, _raw_dir);
    run_command(
        &mut command,
        "failed to stage MSI artifact with administrative extraction",
    )
}

fn stage_exe_payload(artifact_path: &Path, raw_dir: &Path) -> Result<()> {
    if !cfg!(windows) {
        return Err(anyhow!("EXE artifacts are supported only on Windows hosts"));
    }
    stage_exe_payload_with_runner(artifact_path, raw_dir, run_command)
}

pub(crate) fn stage_exe_payload_with_runner<RunCommand>(
    artifact_path: &Path,
    raw_dir: &Path,
    mut run: RunCommand,
) -> Result<()>
where
    RunCommand: FnMut(&mut Command, &str) -> Result<()>,
{
    let mut command = build_exe_extract_command(artifact_path, raw_dir);
    run(
        &mut command,
        "failed to stage EXE artifact via deterministic extraction",
    )
    .map_err(|err| {
        if error_chain_has_not_found(&err) {
            return anyhow!(
                "failed to stage EXE artifact via deterministic extraction: required extraction tool '7z' was not found on PATH; install 7-Zip CLI and ensure '7z' is available, then retry. artifact={} raw_dir={} extraction_command={:?}",
                artifact_path.display(),
                raw_dir.display(),
                command
            );
        }
        anyhow!(
            "failed to stage EXE artifact via deterministic extraction: artifact={} raw_dir={} extraction_command={:?}: {err}",
            artifact_path.display(),
            raw_dir.display(),
            command
        )
    })
}

fn error_chain_has_not_found(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .is_some_and(|io_err| io_err.kind() == io::ErrorKind::NotFound)
    })
}

fn stage_pkg_payload(_artifact_path: &Path, _raw_dir: &Path) -> Result<()> {
    if !cfg!(target_os = "macos") {
        return Err(anyhow!("PKG artifacts are supported only on macOS hosts"));
    }
    let expanded_dir = _raw_dir.join(".crosspack-pkg-expanded");
    stage_pkg_payload_with_hooks(_artifact_path, _raw_dir, &expanded_dir, run_command).map_err(
        |err| {
            if error_chain_has_not_found(&err) {
                return anyhow!(
                    "failed to stage PKG artifact via deterministic extraction: required macOS tool was not found on PATH; ensure 'pkgutil' and 'ditto' are available, then retry. artifact={} raw_dir={} expanded_dir={}: {err}",
                    _artifact_path.display(),
                    _raw_dir.display(),
                    expanded_dir.display()
                );
            }
            anyhow!(
                "failed to stage PKG artifact via deterministic extraction: artifact={} raw_dir={} expanded_dir={}: {err}",
                _artifact_path.display(),
                _raw_dir.display(),
                expanded_dir.display()
            )
        },
    )
}

fn stage_msix_payload(_artifact_path: &Path, _raw_dir: &Path) -> Result<()> {
    if !cfg!(windows) {
        return Err(anyhow!(
            "MSIX artifacts are supported only on Windows hosts"
        ));
    }
    stage_msix_payload_with_runner(_artifact_path, _raw_dir, run_command)
}

fn stage_appx_payload(_artifact_path: &Path, _raw_dir: &Path) -> Result<()> {
    if !cfg!(windows) {
        return Err(anyhow!(
            "APPX artifacts are supported only on Windows hosts"
        ));
    }
    stage_appx_payload_with_runner(_artifact_path, _raw_dir, run_command)
}

pub(crate) fn stage_msix_payload_with_runner<RunCommand>(
    artifact_path: &Path,
    raw_dir: &Path,
    run: RunCommand,
) -> Result<()>
where
    RunCommand: FnMut(&mut Command, &str) -> Result<()>,
{
    stage_windows_unpack_payload_with_runner(
        "MSIX",
        artifact_path,
        raw_dir,
        build_msix_unpack_command,
        run,
    )
}

pub(crate) fn stage_appx_payload_with_runner<RunCommand>(
    artifact_path: &Path,
    raw_dir: &Path,
    run: RunCommand,
) -> Result<()>
where
    RunCommand: FnMut(&mut Command, &str) -> Result<()>,
{
    stage_windows_unpack_payload_with_runner(
        "APPX",
        artifact_path,
        raw_dir,
        build_appx_unpack_command,
        run,
    )
}

fn stage_windows_unpack_payload_with_runner<BuildCommand, RunCommand>(
    artifact_kind: &str,
    artifact_path: &Path,
    raw_dir: &Path,
    mut build_command: BuildCommand,
    mut run: RunCommand,
) -> Result<()>
where
    BuildCommand: FnMut(&Path, &Path) -> Command,
    RunCommand: FnMut(&mut Command, &str) -> Result<()>,
{
    let context = format!("failed to stage {artifact_kind} artifact via deterministic extraction");
    let mut command = build_command(artifact_path, raw_dir);
    run(&mut command, &context).map_err(|err| {
        if error_chain_has_not_found(&err) {
            return anyhow!(
                "{context}: required extraction tool 'makeappx' was not found on PATH; install Windows SDK App Certification Kit tools and ensure 'makeappx' is available, then retry. artifact={} raw_dir={} extraction_command={:?}",
                artifact_path.display(),
                raw_dir.display(),
                command
            );
        }
        anyhow!(
            "{context}: artifact={} raw_dir={} extraction_command={:?}: {err}",
            artifact_path.display(),
            raw_dir.display(),
            command
        )
    })
}

fn stage_dmg_payload(_artifact_path: &Path, _raw_dir: &Path) -> Result<()> {
    if !cfg!(target_os = "macos") {
        return Err(anyhow!("DMG artifacts are supported only on macOS hosts"));
    }

    let mount_point = _raw_dir.join(".crosspack-dmg-mount");
    fs::create_dir_all(&mount_point)
        .with_context(|| format!("failed to create {}", mount_point.display()))?;

    let result = stage_dmg_payload_with_hooks(
        _artifact_path,
        _raw_dir,
        &mount_point,
        run_command,
        copy_dmg_payload,
    );

    let _ = fs::remove_dir_all(&mount_point);
    result
}

pub(crate) fn build_msi_admin_extract_command(artifact_path: &Path, raw_dir: &Path) -> Command {
    let mut command = Command::new("msiexec");
    command
        .arg("/a")
        .arg(artifact_path)
        .arg("/qn")
        .arg(format!("TARGETDIR={}", raw_dir.display()));
    command
}

pub(crate) fn build_exe_extract_command(artifact_path: &Path, raw_dir: &Path) -> Command {
    let mut command = Command::new("7z");
    command
        .arg("x")
        .arg(artifact_path)
        .arg(format!("-o{}", raw_dir.display()))
        .arg("-y");
    command
}

pub(crate) fn build_msix_unpack_command(artifact_path: &Path, raw_dir: &Path) -> Command {
    let mut command = Command::new("makeappx");
    command
        .arg("unpack")
        .arg("/p")
        .arg(artifact_path)
        .arg("/d")
        .arg(raw_dir)
        .arg("/o");
    command
}

pub(crate) fn build_appx_unpack_command(artifact_path: &Path, raw_dir: &Path) -> Command {
    build_msix_unpack_command(artifact_path, raw_dir)
}

pub(crate) fn build_dmg_attach_command(artifact_path: &Path, mount_point: &Path) -> Command {
    let mut command = Command::new("hdiutil");
    command
        .arg("attach")
        .arg(artifact_path)
        .arg("-readonly")
        .arg("-nobrowse")
        .arg("-mountpoint")
        .arg(mount_point);
    command
}

pub(crate) fn build_pkg_expand_command(artifact_path: &Path, expanded_dir: &Path) -> Command {
    let mut command = Command::new("pkgutil");
    command
        .arg("--expand-full")
        .arg(artifact_path)
        .arg(expanded_dir);
    command
}

pub(crate) fn build_pkg_copy_command(expanded_raw_dir: &Path, raw_dir: &Path) -> Command {
    let mut command = Command::new("ditto");
    command.arg(expanded_raw_dir).arg(raw_dir);
    command
}

pub(crate) fn discover_pkg_payload_roots(expanded_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut payload_roots = Vec::new();

    let top_level_payload = expanded_dir.join("Payload");
    if top_level_payload.exists() {
        payload_roots.push(top_level_payload);
    }

    let mut nested_payloads = Vec::new();
    for entry in fs::read_dir(expanded_dir).with_context(|| {
        format!(
            "failed to inspect expanded PKG directory: {}",
            expanded_dir.display()
        )
    })? {
        let entry = entry.with_context(|| {
            format!(
                "failed reading expanded PKG directory entry: {}",
                expanded_dir.display()
            )
        })?;
        if !entry
            .file_type()
            .with_context(|| {
                format!(
                    "failed to inspect expanded PKG entry type: {}",
                    entry.path().display()
                )
            })?
            .is_dir()
        {
            continue;
        }

        let entry_path = entry.path();
        if entry_path.extension().and_then(|value| value.to_str()) != Some("pkg") {
            continue;
        }

        let payload_path = entry_path.join("Payload");
        if payload_path.exists() {
            nested_payloads.push(payload_path);
        }
    }

    nested_payloads.sort();
    payload_roots.extend(nested_payloads);

    if payload_roots.is_empty() {
        return Err(anyhow!(
            "expanded PKG payload not found in {}; expected {} or {}",
            expanded_dir.display(),
            expanded_dir.join("Payload").display(),
            expanded_dir
                .join("<component>.pkg")
                .join("Payload")
                .display()
        ));
    }

    Ok(payload_roots)
}

fn cleanup_pkg_expanded_dir(expanded_dir: &Path) -> Result<()> {
    match fs::remove_dir_all(expanded_dir) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| {
            format!(
                "failed to cleanup expanded PKG payload: {}",
                expanded_dir.display()
            )
        }),
    }
}

pub(crate) fn stage_pkg_payload_with_hooks<RunCommand>(
    artifact_path: &Path,
    raw_dir: &Path,
    expanded_dir: &Path,
    mut run: RunCommand,
) -> Result<()>
where
    RunCommand: FnMut(&mut Command, &str) -> Result<()>,
{
    let mut expand_command = build_pkg_expand_command(artifact_path, expanded_dir);
    let expand_result = run(&mut expand_command, "failed to expand PKG artifact");

    let copy_result = if expand_result.is_ok() {
        Some((|| {
            for payload_root in discover_pkg_payload_roots(expanded_dir)? {
                let mut copy_command = build_pkg_copy_command(&payload_root, raw_dir);
                run(
                    &mut copy_command,
                    "failed to copy expanded PKG payload into staging directory",
                )?;
            }
            Ok(())
        })())
    } else {
        None
    };

    let cleanup_result = cleanup_pkg_expanded_dir(expanded_dir);

    match (expand_result, copy_result, cleanup_result) {
        (Ok(()), Some(Ok(())), Ok(())) => Ok(()),
        (Err(expand_err), _, Ok(())) => Err(expand_err),
        (Ok(()), Some(Err(copy_err)), Ok(())) => Err(copy_err),
        (Ok(()), Some(Ok(())), Err(cleanup_err)) => Err(cleanup_err),
        (Ok(()), Some(Err(copy_err)), Err(cleanup_err)) => Err(anyhow!(
            "failed to copy expanded PKG payload: {copy_err}; additionally failed to cleanup expanded payload {}: {cleanup_err}",
            expanded_dir.display()
        )),
        (Err(expand_err), _, Err(cleanup_err)) => Err(anyhow!(
            "failed to expand PKG artifact: {expand_err}; additionally failed to cleanup expanded payload {}: {cleanup_err}",
            expanded_dir.display()
        )),
        (Ok(()), None, Ok(())) => unreachable!("copy step must run after successful PKG expand"),
        (Ok(()), None, Err(_)) => unreachable!("copy step must run after successful PKG expand"),
    }
}

pub(crate) fn build_dmg_detach_command(mount_point: &Path) -> Command {
    let mut command = Command::new("hdiutil");
    command.arg("detach").arg(mount_point);
    command
}

pub(crate) fn stage_dmg_payload_with_hooks<RunCommand, CopyPayload>(
    artifact_path: &Path,
    raw_dir: &Path,
    mount_point: &Path,
    mut run: RunCommand,
    mut copy_payload: CopyPayload,
) -> Result<()>
where
    RunCommand: FnMut(&mut Command, &str) -> Result<()>,
    CopyPayload: FnMut(&Path, &Path) -> Result<()>,
{
    let mut attach_command = build_dmg_attach_command(artifact_path, mount_point);
    run(&mut attach_command, "failed to attach DMG artifact")?;

    let copy_result = copy_payload(mount_point, raw_dir);

    let mut detach_command = build_dmg_detach_command(mount_point);
    let detach_result = run(&mut detach_command, "failed to detach DMG mount");

    match (copy_result, detach_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(copy_err), Ok(())) => Err(copy_err),
        (Ok(()), Err(detach_err)) => Err(detach_err),
        (Err(copy_err), Err(detach_err)) => Err(anyhow!(
            "failed to copy mounted DMG payload: {copy_err}; additionally failed to detach mount {}: {detach_err}",
            mount_point.display()
        )),
    }
}

pub(crate) fn copy_dmg_payload(mount_point: &Path, raw_dir: &Path) -> Result<()> {
    copy_dir_recursive(mount_point, raw_dir)?;

    let applications_entry = raw_dir.join("Applications");
    let metadata = match fs::symlink_metadata(&applications_entry) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "failed to inspect copied DMG Applications entry: {}",
                    applications_entry.display()
                )
            });
        }
    };

    if metadata.file_type().is_symlink() {
        fs::remove_file(&applications_entry).with_context(|| {
            format!(
                "failed to remove root Applications symlink from DMG payload copy: {}",
                applications_entry.display()
            )
        })?;
    }

    Ok(())
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

pub(crate) fn run_command(command: &mut Command, context_message: &str) -> Result<()> {
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

pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
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

pub(crate) fn strip_rel_components(path: &Path, strip_components: usize) -> Option<PathBuf> {
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
