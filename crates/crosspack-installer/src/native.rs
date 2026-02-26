use anyhow::{anyhow, Context, Result};
use crosspack_core::ArtifactGuiApp;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crate::artifact::{copy_dir_recursive, run_command};
use crate::exposure::{
    normalize_gui_token, normalized_extension, normalized_protocol_scheme, projected_gui_assets,
    render_gui_launcher, sanitize_desktop_list_token, sanitize_gui_metadata_value,
    validated_relative_binary_path,
};
use crate::fs_utils::remove_file_if_exists;
use crate::{
    GuiExposureAsset, GuiNativeRegistrationRecord, NativeSidecarState, NativeUninstallAction,
    PrefixLayout,
};

pub(crate) const MACOS_LSREGISTER_PATH: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

const NATIVE_SIDECAR_VERSION: u32 = 1;

pub fn write_native_sidecar_state(
    layout: &PrefixLayout,
    package_name: &str,
    state: &NativeSidecarState,
) -> Result<PathBuf> {
    let path = layout.gui_native_state_path(package_name);
    if state.uninstall_actions.is_empty() {
        let _ = remove_file_if_exists(&path);
        return Ok(path);
    }

    let mut payload = String::new();
    payload.push_str(&format!("version={}\n", NATIVE_SIDECAR_VERSION));
    for action in &state.uninstall_actions {
        if action.key.contains('\n')
            || action.key.contains('\t')
            || action.kind.contains('\n')
            || action.kind.contains('\t')
            || action.path.contains('\n')
            || action.path.contains('\t')
        {
            return Err(anyhow!(
                "native uninstall action values must not contain tabs or newlines"
            ));
        }
        payload.push_str(&format!(
            "uninstall_action={}\t{}\t{}\n",
            action.key, action.kind, action.path
        ));
    }

    fs::write(&path, payload.as_bytes())
        .with_context(|| format!("failed to write native sidecar state: {}", path.display()))?;
    Ok(path)
}

pub fn read_native_sidecar_state(
    layout: &PrefixLayout,
    package_name: &str,
) -> Result<NativeSidecarState> {
    let path = layout.gui_native_state_path(package_name);
    if !path.exists() {
        return Ok(NativeSidecarState {
            uninstall_actions: Vec::new(),
        });
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read native sidecar state: {}", path.display()))?;
    parse_native_sidecar_state(&raw)
        .with_context(|| format!("failed to parse native sidecar state: {}", path.display()))
}

pub fn read_all_native_sidecar_states(
    layout: &PrefixLayout,
) -> Result<BTreeMap<String, NativeSidecarState>> {
    let dir = layout.installed_state_dir();
    if !dir.exists() {
        return Ok(BTreeMap::new());
    }

    let mut states = BTreeMap::new();
    for entry in fs::read_dir(&dir)
        .with_context(|| format!("failed to read install state directory: {}", dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if !path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.ends_with(".gui-native"))
        {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|v| v.to_str()) else {
            continue;
        };

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read native sidecar state: {}", path.display()))?;
        let state = parse_native_sidecar_state(&raw)
            .with_context(|| format!("failed to parse native sidecar state: {}", path.display()))?;
        states.insert(stem.to_string(), state);
    }

    Ok(states)
}

pub fn clear_native_sidecar_state(layout: &PrefixLayout, package_name: &str) -> Result<()> {
    let path = layout.gui_native_state_path(package_name);
    remove_file_if_exists(&path)?;
    Ok(())
}

pub fn write_gui_native_state(
    layout: &PrefixLayout,
    package_name: &str,
    records: &[GuiNativeRegistrationRecord],
) -> Result<PathBuf> {
    let state = NativeSidecarState {
        uninstall_actions: records
            .iter()
            .cloned()
            .map(NativeUninstallAction::from)
            .collect(),
    };
    write_native_sidecar_state(layout, package_name, &state)
}

pub fn read_gui_native_state(
    layout: &PrefixLayout,
    package_name: &str,
) -> Result<Vec<GuiNativeRegistrationRecord>> {
    let state = read_native_sidecar_state(layout, package_name)?;
    Ok(state
        .uninstall_actions
        .into_iter()
        .map(GuiNativeRegistrationRecord::from)
        .collect())
}

pub fn read_all_gui_native_states(
    layout: &PrefixLayout,
) -> Result<BTreeMap<String, Vec<GuiNativeRegistrationRecord>>> {
    let native_states = read_all_native_sidecar_states(layout)?;
    let mut gui_states = BTreeMap::new();
    for (package_name, state) in native_states {
        let records = state
            .uninstall_actions
            .into_iter()
            .map(GuiNativeRegistrationRecord::from)
            .collect::<Vec<_>>();
        gui_states.insert(package_name, records);
    }
    Ok(gui_states)
}

pub fn clear_gui_native_state(layout: &PrefixLayout, package_name: &str) -> Result<()> {
    clear_native_sidecar_state(layout, package_name)
}

pub fn remove_package_native_gui_registrations_best_effort(
    layout: &PrefixLayout,
    package_name: &str,
) -> Result<Vec<String>> {
    let records = read_gui_native_state(layout, package_name)?;
    if records.is_empty() {
        clear_gui_native_state(layout, package_name)?;
        return Ok(Vec::new());
    }

    let warnings = remove_native_gui_registration_best_effort(&records)?;
    if warnings.is_empty() {
        clear_gui_native_state(layout, package_name)?;
    } else {
        write_gui_native_state(layout, package_name, &records)?;
    }
    Ok(warnings)
}

pub(crate) fn parse_native_sidecar_state(raw: &str) -> Result<NativeSidecarState> {
    let mut version = None;
    let mut uninstall_actions = Vec::new();

    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some((key, value)) = line.split_once('=') else {
            return Err(anyhow!("invalid native sidecar row format: {line}"));
        };
        if key.trim().is_empty() {
            return Err(anyhow!("native sidecar row key must not be empty"));
        }

        match key {
            "version" => {
                let parsed = value
                    .parse::<u32>()
                    .context("native sidecar version must be u32")?;
                version = Some(parsed);
            }
            "uninstall_action" | "record" => {
                uninstall_actions.push(parse_native_uninstall_action(value)?);
            }
            _ => {}
        }
    }

    if let Some(found_version) = version {
        if found_version != NATIVE_SIDECAR_VERSION {
            return Err(anyhow!(
                "unsupported native sidecar version: {found_version}"
            ));
        }
    }

    Ok(NativeSidecarState { uninstall_actions })
}

fn parse_native_uninstall_action(value: &str) -> Result<NativeUninstallAction> {
    let parts = value.split('\t').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(anyhow!("invalid native uninstall action row format"));
    }
    if parts[0].trim().is_empty() {
        return Err(anyhow!("native uninstall action key must not be empty"));
    }
    if parts[1].trim().is_empty() {
        return Err(anyhow!("native uninstall action kind must not be empty"));
    }
    if parts[2].trim().is_empty() {
        return Err(anyhow!("native uninstall action path must not be empty"));
    }
    Ok(NativeUninstallAction {
        key: parts[0].to_string(),
        kind: parts[1].to_string(),
        path: parts[2].to_string(),
    })
}

pub fn run_package_native_uninstall_actions(
    layout: &PrefixLayout,
    package_name: &str,
) -> Result<()> {
    run_native_uninstall_actions(layout, package_name)
}

fn run_native_uninstall_actions(layout: &PrefixLayout, package_name: &str) -> Result<()> {
    let sidecar_state = read_native_sidecar_state(layout, package_name)?;
    for action in &sidecar_state.uninstall_actions {
        execute_native_uninstall_action(action).with_context(|| {
            format!(
                "native uninstall action failed (key='{}', kind='{}', path='{}')",
                action.key, action.kind, action.path
            )
        })?;
    }
    Ok(())
}

fn execute_native_uninstall_action(action: &NativeUninstallAction) -> Result<()> {
    match action.kind.as_str() {
        "desktop-entry" | "start-menu-launcher" => {
            remove_native_uninstall_path(Path::new(&action.path))
        }
        "applications-symlink" => remove_native_applications_symlink_path(Path::new(&action.path)),
        "applications-bundle-copy" => {
            remove_native_uninstall_path_recursive(Path::new(&action.path))
        }
        "registry-key" => remove_native_registry_key(&action.path),
        other => Err(anyhow!(
            "unsupported native uninstall action kind '{other}'"
        )),
    }
}

fn remove_native_uninstall_path(path: &Path) -> Result<()> {
    remove_native_uninstall_path_with_mode(path, false)
}

fn remove_native_uninstall_path_recursive(path: &Path) -> Result<()> {
    remove_native_uninstall_path_with_mode(path, true)
}

fn remove_native_applications_symlink_path(path: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "failed to inspect native uninstall path: {}",
                    path.display()
                )
            });
        }
    };

    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return remove_native_uninstall_path(path);
    }

    Ok(())
}

fn remove_native_uninstall_path_with_mode(path: &Path, recursive: bool) -> Result<()> {
    let remove_result = match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.is_dir() {
                if recursive {
                    fs::remove_dir_all(path)
                } else {
                    fs::remove_dir(path)
                }
            } else {
                fs::remove_file(path)
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "failed to inspect native uninstall path: {}",
                    path.display()
                )
            });
        }
    };

    match remove_result {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err)
            .with_context(|| format!("failed to remove native uninstall path: {}", path.display())),
    }
}

fn remove_native_registry_key(path: &str) -> Result<()> {
    if !cfg!(windows) {
        return Err(anyhow!(
            "native uninstall action kind 'registry-key' is supported only on Windows hosts"
        ));
    }

    if !windows_registry_key_exists(path)? {
        return Ok(());
    }

    let mut command = Command::new("reg");
    command.arg("delete").arg(path).arg("/f");
    match run_command(&mut command, "failed to remove Windows registry key") {
        Ok(()) => Ok(()),
        Err(err) => {
            if !windows_registry_key_exists(path)? {
                return Ok(());
            }
            Err(err)
        }
    }
}

fn windows_registry_key_exists(path: &str) -> Result<bool> {
    let output = Command::new("reg")
        .arg("query")
        .arg(path)
        .status()
        .with_context(|| format!("failed to query Windows registry key: {path}"))?;
    Ok(output.success())
}

pub fn register_native_gui_app_best_effort(
    package_name: &str,
    app: &ArtifactGuiApp,
    install_root: &Path,
    previous_records: &[GuiNativeRegistrationRecord],
) -> Result<(Vec<GuiNativeRegistrationRecord>, Vec<String>)> {
    register_native_gui_app_best_effort_with_executor(
        package_name,
        app,
        install_root,
        previous_records,
        run_command,
    )
}

pub fn remove_native_gui_registration_best_effort(
    records: &[GuiNativeRegistrationRecord],
) -> Result<Vec<String>> {
    remove_native_gui_registration_best_effort_with_executor(records, run_command)
}

pub(crate) fn register_native_gui_app_best_effort_with_executor<RunCommand>(
    package_name: &str,
    app: &ArtifactGuiApp,
    install_root: &Path,
    previous_records: &[GuiNativeRegistrationRecord],
    mut run_command_executor: RunCommand,
) -> Result<(Vec<GuiNativeRegistrationRecord>, Vec<String>)>
where
    RunCommand: FnMut(&mut Command, &str) -> Result<()>,
{
    let source_rel = validated_relative_binary_path(&app.exec)
        .with_context(|| format!("gui app '{}' exec path is invalid", app.app_id))?;
    let source_path = install_root.join(source_rel);
    if !source_path.exists() {
        return Err(anyhow!(
            "declared gui app exec path '{}' was not found in install root: {}",
            app.exec,
            source_path.display()
        ));
    }

    let projected_assets = projected_gui_assets(package_name, app)?;
    let mut records = Vec::new();
    let mut warnings = Vec::new();

    if cfg!(target_os = "linux") {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            warnings.push(
                "native GUI registration warning: HOME is not set; skipped Linux desktop registration"
                    .to_string(),
            );
            return Ok((records, warnings));
        };

        let applications_dir = project_linux_user_applications_dir(&home);
        if let Err(err) = fs::create_dir_all(&applications_dir) {
            warnings.push(format!(
                "native GUI registration warning: failed to create Linux user applications dir {}: {}",
                applications_dir.display(),
                err
            ));
            return Ok((records, warnings));
        }

        let desktop_path = applications_dir.join(native_gui_launcher_filename(package_name, app));
        let desktop_entry = render_linux_native_desktop_entry(app, &source_path);
        if let Err(err) = fs::write(&desktop_path, desktop_entry.as_bytes()) {
            warnings.push(format!(
                "native GUI registration warning: failed to write Linux desktop entry {}: {}",
                desktop_path.display(),
                err
            ));
            return Ok((records, warnings));
        }

        for asset in &projected_assets {
            records.push(GuiNativeRegistrationRecord {
                key: asset.key.clone(),
                kind: "desktop-entry".to_string(),
                path: desktop_path.display().to_string(),
            });
        }

        let mut refresh = Command::new("update-desktop-database");
        refresh.arg(&applications_dir);
        if let Err(err) = run_command_executor(
            &mut refresh,
            "failed to refresh Linux desktop entry database",
        ) {
            warnings.push(format!("native GUI registration warning: {err}"));
        }

        return Ok((records, warnings));
    }

    if cfg!(windows) {
        let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) else {
            warnings.push(
                "native GUI registration warning: APPDATA is not set; skipped Windows GUI registration"
                    .to_string(),
            );
            return Ok((records, warnings));
        };

        let start_menu_dir = project_windows_start_menu_programs_dir(&appdata);
        if let Err(err) = fs::create_dir_all(&start_menu_dir) {
            warnings.push(format!(
                "native GUI registration warning: failed to create Windows Start Menu programs dir {}: {}",
                start_menu_dir.display(),
                err
            ));
            return Ok((records, warnings));
        }

        let launcher_path = start_menu_dir.join(format!(
            "{}.cmd",
            normalize_gui_token(&format!("{}-{}", package_name, app.app_id))
        ));
        let launcher = render_gui_launcher(app, &source_path);
        if let Err(err) = fs::write(&launcher_path, launcher.as_bytes()) {
            warnings.push(format!(
                "native GUI registration warning: failed to write Windows Start Menu launcher {}: {}",
                launcher_path.display(),
                err
            ));
            return Ok((records, warnings));
        }

        for asset in projected_assets
            .iter()
            .filter(|asset| asset.key.starts_with("app:"))
        {
            records.push(GuiNativeRegistrationRecord {
                key: asset.key.clone(),
                kind: "start-menu-launcher".to_string(),
                path: launcher_path.display().to_string(),
            });
        }

        let open_command = format!("\"{}\" \"%1\"", source_path.display());
        for protocol in &app.protocols {
            let scheme = normalized_protocol_scheme(&protocol.scheme)?;
            let key_path = format!(r"HKCU\Software\Classes\{scheme}");
            let mut register_scheme = Command::new("reg");
            register_scheme
                .arg("add")
                .arg(&key_path)
                .arg("/ve")
                .arg("/d")
                .arg(format!("URL:{}", app.display_name.trim()))
                .arg("/f");
            if let Err(err) = run_command_executor(
                &mut register_scheme,
                "failed to register Windows protocol class",
            ) {
                warnings.push(format!("native GUI registration warning: {err}"));
            }

            let mut protocol_marker = Command::new("reg");
            protocol_marker
                .arg("add")
                .arg(&key_path)
                .arg("/v")
                .arg("URL Protocol")
                .arg("/d")
                .arg("")
                .arg("/f");
            if let Err(err) = run_command_executor(
                &mut protocol_marker,
                "failed to set Windows protocol marker",
            ) {
                warnings.push(format!("native GUI registration warning: {err}"));
            }

            let mut open_key = Command::new("reg");
            open_key
                .arg("add")
                .arg(format!(r"{key_path}\shell\open\command"))
                .arg("/ve")
                .arg("/d")
                .arg(&open_command)
                .arg("/f");
            if let Err(err) = run_command_executor(
                &mut open_key,
                "failed to register Windows protocol open command",
            ) {
                warnings.push(format!("native GUI registration warning: {err}"));
            }

            records.push(GuiNativeRegistrationRecord {
                key: format!("protocol:{scheme}"),
                kind: "registry-key".to_string(),
                path: key_path,
            });
        }

        for association in &app.file_associations {
            for extension in &association.extensions {
                let normalized = normalized_extension(extension)?;
                let ext_key = format!(r"HKCU\Software\Classes\{normalized}");
                let class_key = format!(
                    r"HKCU\Software\Classes\Crosspack.{}.{}.file",
                    normalize_gui_token(package_name),
                    normalize_gui_token(&app.app_id)
                );

                let mut map_extension = Command::new("reg");
                map_extension
                    .arg("add")
                    .arg(&ext_key)
                    .arg("/ve")
                    .arg("/d")
                    .arg(class_key.clone())
                    .arg("/f");
                if let Err(err) = run_command_executor(
                    &mut map_extension,
                    "failed to register Windows file extension mapping",
                ) {
                    warnings.push(format!("native GUI registration warning: {err}"));
                }

                let mut class_open = Command::new("reg");
                class_open
                    .arg("add")
                    .arg(format!(r"{class_key}\shell\open\command"))
                    .arg("/ve")
                    .arg("/d")
                    .arg(&open_command)
                    .arg("/f");
                if let Err(err) = run_command_executor(
                    &mut class_open,
                    "failed to register Windows file extension open command",
                ) {
                    warnings.push(format!("native GUI registration warning: {err}"));
                }

                records.push(GuiNativeRegistrationRecord {
                    key: format!("extension:{normalized}"),
                    kind: "registry-key".to_string(),
                    path: ext_key,
                });
                records.push(GuiNativeRegistrationRecord {
                    key: format!("mime:{}", association.mime_type.trim().to_ascii_lowercase()),
                    kind: "registry-key".to_string(),
                    path: class_key,
                });
            }
        }

        return Ok((records, warnings));
    }

    if cfg!(target_os = "macos") {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            warnings.push(
                "native GUI registration warning: HOME is not set; skipped macOS GUI registration"
                    .to_string(),
            );
            return Ok((records, warnings));
        };

        let registration_source_path = macos_registration_source_path(install_root, &source_path);
        let app_name = registration_source_path
            .file_name()
            .ok_or_else(|| anyhow!("gui app '{}' has invalid executable path", app.app_id))?;
        let destination_candidates = macos_registration_destination_candidates(&home, app_name);
        let (mut macos_records, mut macos_warnings) =
            register_macos_native_gui_registration_with_executor(
                &projected_assets,
                &registration_source_path,
                destination_candidates,
                previous_records,
                &mut run_command_executor,
            );
        records.append(&mut macos_records);
        warnings.append(&mut macos_warnings);

        return Ok((records, warnings));
    }

    warnings.push("native GUI registration warning: host platform is not supported".to_string());
    Ok((records, warnings))
}

fn remove_native_gui_registration_best_effort_with_executor<RunCommand>(
    records: &[GuiNativeRegistrationRecord],
    mut run_command_executor: RunCommand,
) -> Result<Vec<String>>
where
    RunCommand: FnMut(&mut Command, &str) -> Result<()>,
{
    let mut warnings = Vec::new();
    let mut removed_files = HashSet::new();

    for record in records {
        match record.kind.as_str() {
            "desktop-entry" | "start-menu-launcher" => {
                let path = PathBuf::from(&record.path);
                if !removed_files.insert(path.clone()) {
                    continue;
                }
                if let Err(err) = remove_native_uninstall_path(&path) {
                    warnings.push(format!(
                        "native GUI deregistration warning: failed to remove '{}': {}",
                        path.display(),
                        err
                    ));
                }
            }
            "applications-symlink" => {
                let path = PathBuf::from(&record.path);
                if !removed_files.insert(path.clone()) {
                    continue;
                }
                if let Err(err) = remove_native_applications_symlink_path(&path) {
                    warnings.push(format!(
                        "native GUI deregistration warning: failed to remove '{}': {}",
                        path.display(),
                        err
                    ));
                }
            }
            "applications-bundle-copy" => {
                let path = PathBuf::from(&record.path);
                if !removed_files.insert(path.clone()) {
                    continue;
                }
                if let Err(err) = remove_native_uninstall_path_recursive(&path) {
                    warnings.push(format!(
                        "native GUI deregistration warning: failed to remove '{}': {}",
                        path.display(),
                        err
                    ));
                }
            }
            "registry-key" => {
                if !cfg!(windows) {
                    warnings.push(format!(
                        "native GUI deregistration warning: skipped Windows registry cleanup '{}' on non-Windows host",
                        record.path
                    ));
                    continue;
                }

                let mut command = Command::new("reg");
                command.arg("delete").arg(&record.path).arg("/f");
                if let Err(err) =
                    run_command_executor(&mut command, "failed to remove Windows registry key")
                {
                    warnings.push(format!("native GUI deregistration warning: {err}"));
                }
            }
            other => warnings.push(format!(
                "native GUI deregistration warning: unsupported registration kind '{}' for key '{}'",
                other, record.key
            )),
        }
    }

    Ok(warnings)
}

pub(crate) fn project_linux_user_applications_dir(home: &Path) -> PathBuf {
    home.join(".local").join("share").join("applications")
}

pub(crate) fn project_windows_start_menu_programs_dir(appdata: &Path) -> PathBuf {
    appdata
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
}

pub(crate) fn project_macos_user_applications_dir(home: &Path) -> PathBuf {
    home.join("Applications")
}

pub(crate) fn macos_registration_destination_candidates(
    home: &Path,
    app_name: &std::ffi::OsStr,
) -> [PathBuf; 2] {
    [
        PathBuf::from("/Applications").join(app_name),
        project_macos_user_applications_dir(home).join(app_name),
    ]
}

pub(crate) fn select_macos_registration_destination(
    destination_candidates: [PathBuf; 2],
    previous_records: &[GuiNativeRegistrationRecord],
) -> (Option<PathBuf>, Vec<String>) {
    let mut warnings = Vec::new();

    for destination in destination_candidates {
        match prepare_macos_registration_destination(&destination, previous_records) {
            Ok(()) => return (Some(destination), warnings),
            Err(warning) => warnings.push(warning),
        }
    }

    (None, warnings)
}

fn register_macos_native_gui_registration_with_executor<RunCommand>(
    projected_assets: &[GuiExposureAsset],
    registration_source_path: &Path,
    destination_candidates: [PathBuf; 2],
    previous_records: &[GuiNativeRegistrationRecord],
    run_command_executor: &mut RunCommand,
) -> (Vec<GuiNativeRegistrationRecord>, Vec<String>)
where
    RunCommand: FnMut(&mut Command, &str) -> Result<()>,
{
    register_macos_native_gui_registration_with_executor_and_creator(
        projected_assets,
        registration_source_path,
        destination_candidates,
        previous_records,
        run_command_executor,
        create_macos_application_symlink,
    )
}

pub(crate) fn register_macos_native_gui_registration_with_executor_and_creator<
    RunCommand,
    CreateSymlink,
>(
    projected_assets: &[GuiExposureAsset],
    registration_source_path: &Path,
    destination_candidates: [PathBuf; 2],
    previous_records: &[GuiNativeRegistrationRecord],
    run_command_executor: &mut RunCommand,
    create_symlink: CreateSymlink,
) -> (Vec<GuiNativeRegistrationRecord>, Vec<String>)
where
    RunCommand: FnMut(&mut Command, &str) -> Result<()>,
    CreateSymlink: FnMut(&Path, &Path) -> io::Result<()>,
{
    let mut records = Vec::new();
    let uses_bundle_copy = is_macos_app_bundle_path(registration_source_path);
    let (selected_destination, mut warnings) = if uses_bundle_copy {
        register_macos_application_bundle_copy(
            registration_source_path,
            destination_candidates,
            previous_records,
        )
    } else {
        register_macos_application_symlink_with_creator(
            registration_source_path,
            destination_candidates,
            previous_records,
            create_symlink,
        )
    };
    let Some(link_path) = selected_destination else {
        return (records, warnings);
    };
    let native_kind = if uses_bundle_copy {
        "applications-bundle-copy"
    } else {
        "applications-symlink"
    };

    for asset in projected_assets {
        records.push(GuiNativeRegistrationRecord {
            key: asset.key.clone(),
            kind: native_kind.to_string(),
            path: link_path.display().to_string(),
        });
    }

    let mut refresh = Command::new(MACOS_LSREGISTER_PATH);
    refresh.arg("-f").arg(&link_path);
    if let Err(err) = run_command_executor(
        &mut refresh,
        "failed to refresh macOS LaunchServices registry",
    ) {
        warnings.push(format!("native GUI registration warning: {err}"));
    }

    (records, warnings)
}

fn register_macos_application_bundle_copy(
    registration_source_path: &Path,
    destination_candidates: [PathBuf; 2],
    previous_records: &[GuiNativeRegistrationRecord],
) -> (Option<PathBuf>, Vec<String>) {
    let [system_destination, user_destination] = destination_candidates;
    let mut warnings = Vec::new();

    let (selected_destination, mut selection_warnings) = select_macos_registration_destination(
        [system_destination.clone(), user_destination.clone()],
        previous_records,
    );
    warnings.append(&mut selection_warnings);

    let Some(selected_destination) = selected_destination else {
        return (None, warnings);
    };

    match write_macos_registration_bundle_copy(registration_source_path, &selected_destination) {
        Ok(()) => return (Some(selected_destination), warnings),
        Err(warning) => warnings.push(warning),
    }

    if selected_destination != system_destination || user_destination == system_destination {
        return (None, warnings);
    }

    match prepare_macos_registration_destination(&user_destination, previous_records) {
        Ok(()) => {}
        Err(warning) => {
            warnings.push(warning);
            return (None, warnings);
        }
    }

    match write_macos_registration_bundle_copy(registration_source_path, &user_destination) {
        Ok(()) => (Some(user_destination), warnings),
        Err(warning) => {
            warnings.push(warning);
            (None, warnings)
        }
    }
}

pub(crate) fn register_macos_application_symlink_with_creator<CreateSymlink>(
    registration_source_path: &Path,
    destination_candidates: [PathBuf; 2],
    previous_records: &[GuiNativeRegistrationRecord],
    mut create_symlink: CreateSymlink,
) -> (Option<PathBuf>, Vec<String>)
where
    CreateSymlink: FnMut(&Path, &Path) -> io::Result<()>,
{
    let [system_destination, user_destination] = destination_candidates;
    let mut warnings = Vec::new();

    let (selected_destination, mut selection_warnings) = select_macos_registration_destination(
        [system_destination.clone(), user_destination.clone()],
        previous_records,
    );
    warnings.append(&mut selection_warnings);

    let Some(selected_destination) = selected_destination else {
        return (None, warnings);
    };

    match write_macos_registration_symlink_with_creator(
        registration_source_path,
        &selected_destination,
        &mut create_symlink,
    ) {
        Ok(()) => return (Some(selected_destination), warnings),
        Err(warning) => warnings.push(warning),
    }

    if selected_destination != system_destination || user_destination == system_destination {
        return (None, warnings);
    }

    match prepare_macos_registration_destination(&user_destination, previous_records) {
        Ok(()) => {}
        Err(warning) => {
            warnings.push(warning);
            return (None, warnings);
        }
    }

    match write_macos_registration_symlink_with_creator(
        registration_source_path,
        &user_destination,
        &mut create_symlink,
    ) {
        Ok(()) => (Some(user_destination), warnings),
        Err(warning) => {
            warnings.push(warning);
            (None, warnings)
        }
    }
}

fn prepare_macos_registration_destination(
    destination: &Path,
    previous_records: &[GuiNativeRegistrationRecord],
) -> std::result::Result<(), String> {
    let Some(applications_dir) = destination.parent() else {
        return Err(format!(
            "native GUI registration warning: invalid macOS applications destination {}",
            destination.display()
        ));
    };

    if let Err(err) = fs::create_dir_all(applications_dir) {
        return Err(format!(
            "native GUI registration warning: failed to prepare macOS applications dir {}: {}",
            applications_dir.display(),
            err
        ));
    }

    if destination.exists() && !macos_previous_records_include_path(previous_records, destination) {
        return Err(format!(
            "native GUI registration warning: refusing to overwrite unmanaged macOS app bundle {}",
            destination.display()
        ));
    }

    Ok(())
}

fn write_macos_registration_symlink_with_creator<CreateSymlink>(
    registration_source_path: &Path,
    link_path: &Path,
    create_symlink: &mut CreateSymlink,
) -> std::result::Result<(), String>
where
    CreateSymlink: FnMut(&Path, &Path) -> io::Result<()>,
{
    if link_path.exists() {
        let remove_result = match fs::symlink_metadata(link_path) {
            Ok(metadata) => {
                if metadata.is_dir() {
                    fs::remove_dir(link_path)
                } else {
                    fs::remove_file(link_path)
                }
            }
            Err(err) => Err(err),
        };
        if let Err(err) = remove_result {
            return Err(format!(
                "native GUI registration warning: failed to replace existing macOS application link {}: {}",
                link_path.display(),
                err
            ));
        }
    }

    if let Err(err) = create_symlink(registration_source_path, link_path) {
        return Err(format!(
            "native GUI registration warning: failed to create macOS application symlink {} -> {}: {}",
            link_path.display(),
            registration_source_path.display(),
            err
        ));
    }

    Ok(())
}

fn write_macos_registration_bundle_copy(
    registration_source_path: &Path,
    destination_path: &Path,
) -> std::result::Result<(), String> {
    if !registration_source_path.is_dir() {
        return Err(format!(
            "native GUI registration warning: macOS app bundle source is not a directory {}",
            registration_source_path.display()
        ));
    }

    if destination_path.exists() {
        let remove_result = match fs::symlink_metadata(destination_path) {
            Ok(metadata) => {
                if metadata.is_dir() {
                    fs::remove_dir_all(destination_path)
                } else {
                    fs::remove_file(destination_path)
                }
            }
            Err(err) => Err(err),
        };
        if let Err(err) = remove_result {
            return Err(format!(
                "native GUI registration warning: failed to replace existing macOS application bundle {}: {}",
                destination_path.display(),
                err
            ));
        }
    }

    if let Err(err) = copy_dir_recursive(registration_source_path, destination_path) {
        return Err(format!(
            "native GUI registration warning: failed to copy macOS application bundle {} -> {}: {}",
            registration_source_path.display(),
            destination_path.display(),
            err
        ));
    }

    Ok(())
}

#[cfg(unix)]
fn create_macos_application_symlink(
    registration_source_path: &Path,
    link_path: &Path,
) -> io::Result<()> {
    std::os::unix::fs::symlink(registration_source_path, link_path)
}

#[cfg(not(unix))]
fn create_macos_application_symlink(
    _registration_source_path: &Path,
    _link_path: &Path,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "symlink creation is unsupported on this host",
    ))
}

fn macos_previous_records_include_path(
    previous_records: &[GuiNativeRegistrationRecord],
    destination: &Path,
) -> bool {
    previous_records.iter().any(|record| {
        record.kind.starts_with("applications-") && Path::new(&record.path) == destination
    })
}

pub(crate) fn macos_registration_source_path(install_root: &Path, source_path: &Path) -> PathBuf {
    let Ok(relative) = source_path.strip_prefix(install_root) else {
        return source_path.to_path_buf();
    };

    let mut bundle_root = PathBuf::new();
    for component in relative.components() {
        let Component::Normal(value) = component else {
            continue;
        };
        bundle_root.push(value);
        if Path::new(value)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("app"))
            .unwrap_or(false)
        {
            return install_root.join(bundle_root);
        }
    }

    source_path.to_path_buf()
}

fn is_macos_app_bundle_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("app"))
        .unwrap_or(false)
}

fn native_gui_launcher_filename(package_name: &str, app: &ArtifactGuiApp) -> String {
    format!(
        "{}--{}.desktop",
        normalize_gui_token(package_name),
        normalize_gui_token(&app.app_id)
    )
}

fn render_linux_native_desktop_entry(app: &ArtifactGuiApp, source_path: &Path) -> String {
    let mut mime_entries = app
        .file_associations
        .iter()
        .map(|assoc| sanitize_desktop_list_token(&assoc.mime_type))
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();
    mime_entries.extend(app.protocols.iter().map(|protocol| {
        format!(
            "x-scheme-handler/{}",
            sanitize_desktop_list_token(&protocol.scheme)
        )
    }));

    let mut desktop = String::new();
    desktop.push_str("[Desktop Entry]\n");
    desktop.push_str("Type=Application\n");
    desktop.push_str(&format!(
        "Name={}\n",
        sanitize_gui_metadata_value(&app.display_name)
    ));
    desktop.push_str(&format!("Exec=\"{}\" %U\n", source_path.display()));
    if let Some(icon) = &app.icon {
        desktop.push_str(&format!("Icon={}\n", sanitize_gui_metadata_value(icon)));
    }
    if !app.categories.is_empty() {
        let categories = app
            .categories
            .iter()
            .map(|category| sanitize_desktop_list_token(category))
            .filter(|category| !category.is_empty())
            .collect::<Vec<_>>();
        if !categories.is_empty() {
            desktop.push_str(&format!("Categories={};\n", categories.join(";")));
        }
    }
    if !mime_entries.is_empty() {
        desktop.push_str(&format!("MimeType={};\n", mime_entries.join(";")));
    }
    desktop
}
