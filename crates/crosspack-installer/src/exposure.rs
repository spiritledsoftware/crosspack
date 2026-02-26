use anyhow::{anyhow, Context, Result};
use crosspack_core::{ArtifactCompletionShell, ArtifactGuiApp};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::fs_utils::remove_file_if_exists;
use crate::{GuiExposureAsset, PrefixLayout};

pub fn write_gui_exposure_state(
    layout: &PrefixLayout,
    package_name: &str,
    assets: &[GuiExposureAsset],
) -> Result<PathBuf> {
    let path = layout.gui_state_path(package_name);
    if assets.is_empty() {
        let _ = remove_file_if_exists(&path);
        return Ok(path);
    }

    let mut payload = String::new();
    for asset in assets {
        if asset.key.contains('\n')
            || asset.key.contains('\t')
            || asset.rel_path.contains('\n')
            || asset.rel_path.contains('\t')
        {
            return Err(anyhow!(
                "gui exposure state values must not contain tabs or newlines"
            ));
        }
        payload.push_str(&format!("asset={}\t{}\n", asset.key, asset.rel_path));
    }

    fs::write(&path, payload.as_bytes())
        .with_context(|| format!("failed to write gui exposure state: {}", path.display()))?;
    Ok(path)
}

pub fn read_gui_exposure_state(
    layout: &PrefixLayout,
    package_name: &str,
) -> Result<Vec<GuiExposureAsset>> {
    let path = layout.gui_state_path(package_name);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read gui exposure state: {}", path.display()))?;
    parse_gui_exposure_state(&raw)
        .with_context(|| format!("failed to parse gui exposure state: {}", path.display()))
}

pub fn read_all_gui_exposure_states(
    layout: &PrefixLayout,
) -> Result<BTreeMap<String, Vec<GuiExposureAsset>>> {
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
        if path.extension().and_then(|v| v.to_str()) != Some("gui") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|v| v.to_str()) else {
            continue;
        };

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read gui exposure state: {}", path.display()))?;
        let assets = parse_gui_exposure_state(&raw)
            .with_context(|| format!("failed to parse gui exposure state: {}", path.display()))?;
        states.insert(stem.to_string(), assets);
    }

    Ok(states)
}

pub fn clear_gui_exposure_state(layout: &PrefixLayout, package_name: &str) -> Result<()> {
    let path = layout.gui_state_path(package_name);
    remove_file_if_exists(&path)?;
    Ok(())
}

fn parse_gui_exposure_state(raw: &str) -> Result<Vec<GuiExposureAsset>> {
    let mut assets = Vec::new();
    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some(payload) = line.strip_prefix("asset=") else {
            continue;
        };
        let Some((key, rel_path)) = payload.split_once('\t') else {
            return Err(anyhow!("invalid gui state row format"));
        };
        if key.trim().is_empty() {
            return Err(anyhow!("gui exposure key must not be empty"));
        }
        validated_relative_gui_storage_path(rel_path)?;
        assets.push(GuiExposureAsset {
            key: key.to_string(),
            rel_path: rel_path.to_string(),
        });
    }
    Ok(assets)
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
    let source_path = resolve_binary_source_path(install_root, binary_rel_path)?;

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

pub fn projected_exposed_completion_path(
    package_name: &str,
    shell: ArtifactCompletionShell,
    completion_rel_path: &str,
) -> Result<String> {
    let relative = validated_relative_completion_source_path(completion_rel_path)?;
    let normalized_package = normalize_completion_token(package_name);
    let normalized_path = normalize_completion_source_path(relative);
    Ok(format!(
        "packages/{}/{}--{}",
        shell.as_str(),
        normalized_package,
        normalized_path
    ))
}

pub fn exposed_completion_path(
    layout: &PrefixLayout,
    completion_storage_rel_path: &str,
) -> Result<PathBuf> {
    let relative = validated_relative_completion_storage_path(completion_storage_rel_path)?;
    Ok(layout.completions_dir().join(relative))
}

pub fn expose_completion(
    layout: &PrefixLayout,
    install_root: &Path,
    package_name: &str,
    shell: ArtifactCompletionShell,
    completion_rel_path: &str,
) -> Result<String> {
    let source_rel = validated_relative_completion_source_path(completion_rel_path)?;
    let source_path = install_root.join(source_rel);
    if !source_path.exists() {
        return Err(anyhow!(
            "declared completion path '{}' was not found in install root: {}",
            completion_rel_path,
            source_path.display()
        ));
    }

    let metadata = fs::metadata(&source_path).with_context(|| {
        format!(
            "failed to inspect completion path: {}",
            source_path.display()
        )
    })?;
    if !metadata.is_file() {
        return Err(anyhow!(
            "declared completion path '{}' must be a file: {}",
            completion_rel_path,
            source_path.display()
        ));
    }

    let storage_rel_path =
        projected_exposed_completion_path(package_name, shell, completion_rel_path)?;
    let destination = exposed_completion_path(layout, &storage_rel_path)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create completion dir: {}", parent.display()))?;
    }
    if destination.exists() {
        fs::remove_file(&destination).with_context(|| {
            format!(
                "failed to replace existing completion file: {}",
                destination.display()
            )
        })?;
    }

    fs::copy(&source_path, &destination).with_context(|| {
        format!(
            "failed to expose completion file {} -> {}",
            source_path.display(),
            destination.display()
        )
    })?;
    Ok(storage_rel_path)
}

pub fn remove_exposed_completion(
    layout: &PrefixLayout,
    completion_storage_rel_path: &str,
) -> Result<()> {
    let destination = exposed_completion_path(layout, completion_storage_rel_path)?;
    if !destination.exists() {
        return Ok(());
    }

    fs::remove_file(&destination).with_context(|| {
        format!(
            "failed to remove exposed completion file: {}",
            destination.display()
        )
    })?;

    prune_empty_completion_dirs(layout, destination.parent())?;
    Ok(())
}

pub fn gui_asset_path(layout: &PrefixLayout, gui_storage_rel_path: &str) -> Result<PathBuf> {
    let relative = validated_relative_gui_storage_path(gui_storage_rel_path)?;
    Ok(layout.gui_dir().join(relative))
}

pub fn projected_gui_assets(
    package_name: &str,
    app: &ArtifactGuiApp,
) -> Result<Vec<GuiExposureAsset>> {
    if app.app_id.trim().is_empty() {
        return Err(anyhow!("gui app id must not be empty"));
    }
    if app.display_name.trim().is_empty() {
        return Err(anyhow!(
            "gui app '{}' display_name must not be empty",
            app.app_id
        ));
    }
    validated_relative_binary_path(&app.exec)
        .with_context(|| format!("gui app '{}' exec path is invalid", app.app_id))?;

    let package_token = normalize_gui_token(package_name);
    let app_token = normalize_gui_token(&app.app_id);
    let launcher_rel = format!(
        "launchers/{package_token}--{app_token}.{}",
        gui_launcher_extension()
    );
    let handler_rel = format!("handlers/{package_token}--{app_token}.meta");

    let mut assets = Vec::new();
    let mut seen_keys = HashSet::new();
    let mut push_asset = |key: String, rel_path: &str| -> Result<()> {
        if !seen_keys.insert(key.clone()) {
            return Err(anyhow!(
                "duplicate gui ownership key declaration '{}': app '{}'",
                key,
                app.app_id
            ));
        }
        assets.push(GuiExposureAsset {
            key,
            rel_path: rel_path.to_string(),
        });
        Ok(())
    };

    push_asset(
        format!("app:{}", app.app_id.trim().to_ascii_lowercase()),
        &launcher_rel,
    )?;
    push_asset(
        format!("handler:{}", app.app_id.trim().to_ascii_lowercase()),
        &handler_rel,
    )?;

    for protocol in &app.protocols {
        let scheme = normalized_protocol_scheme(&protocol.scheme)
            .with_context(|| format!("gui app '{}' has invalid protocol scheme", app.app_id))?;
        push_asset(format!("protocol:{scheme}"), &handler_rel)?;
    }

    for association in &app.file_associations {
        let mime = association.mime_type.trim().to_ascii_lowercase();
        if mime.is_empty() {
            return Err(anyhow!(
                "gui app '{}' file association mime_type must not be empty",
                app.app_id
            ));
        }
        push_asset(format!("mime:{mime}"), &handler_rel)?;
        for extension in &association.extensions {
            let normalized = normalized_extension(extension).with_context(|| {
                format!(
                    "gui app '{}' has invalid file association extension",
                    app.app_id
                )
            })?;
            push_asset(format!("extension:{normalized}"), &handler_rel)?;
        }
    }

    Ok(assets)
}

pub fn expose_gui_app(
    layout: &PrefixLayout,
    install_root: &Path,
    package_name: &str,
    app: &ArtifactGuiApp,
) -> Result<Vec<GuiExposureAsset>> {
    let projected = projected_gui_assets(package_name, app)?;
    let launcher_asset = projected
        .iter()
        .find(|asset| asset.key.starts_with("app:"))
        .cloned()
        .ok_or_else(|| anyhow!("missing projected launcher asset for app '{}'", app.app_id))?;
    let handler_asset = projected
        .iter()
        .find(|asset| asset.key.starts_with("handler:"))
        .cloned()
        .ok_or_else(|| anyhow!("missing projected handler asset for app '{}'", app.app_id))?;

    let source_rel = validated_relative_binary_path(&app.exec)?;
    let source_path = install_root.join(source_rel);
    if !source_path.exists() {
        return Err(anyhow!(
            "declared gui app exec path '{}' was not found in install root: {}",
            app.exec,
            source_path.display()
        ));
    }

    let launcher_path = gui_asset_path(layout, &launcher_asset.rel_path)?;
    if let Some(parent) = launcher_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create gui launcher dir: {}", parent.display()))?;
    }

    let launcher = render_gui_launcher(app, &source_path);
    fs::write(&launcher_path, launcher.as_bytes())
        .with_context(|| format!("failed writing gui launcher: {}", launcher_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&launcher_path)
            .with_context(|| {
                format!(
                    "failed to inspect gui launcher: {}",
                    launcher_path.display()
                )
            })?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&launcher_path, permissions).with_context(|| {
            format!(
                "failed setting gui launcher permissions: {}",
                launcher_path.display()
            )
        })?;
    }

    let handler_path = gui_asset_path(layout, &handler_asset.rel_path)?;
    if let Some(parent) = handler_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create gui handler dir: {}", parent.display()))?;
    }

    let mut metadata = String::new();
    metadata.push_str(&format!(
        "app_id={}\n",
        sanitize_gui_metadata_value(&app.app_id)
    ));
    metadata.push_str(&format!(
        "display_name={}\n",
        sanitize_gui_metadata_value(&app.display_name)
    ));
    metadata.push_str(&format!(
        "exec={}\n",
        sanitize_gui_metadata_value(&app.exec)
    ));
    if let Some(icon) = &app.icon {
        metadata.push_str(&format!("icon={}\n", sanitize_gui_metadata_value(icon)));
    }
    for category in &app.categories {
        metadata.push_str(&format!(
            "category={}\n",
            sanitize_gui_metadata_value(category)
        ));
    }
    for protocol in &app.protocols {
        metadata.push_str(&format!(
            "protocol={}\n",
            sanitize_gui_metadata_value(&protocol.scheme)
        ));
    }
    for association in &app.file_associations {
        metadata.push_str(&format!(
            "mime={}\n",
            sanitize_gui_metadata_value(&association.mime_type)
        ));
        for extension in &association.extensions {
            metadata.push_str(&format!(
                "extension={}\n",
                sanitize_gui_metadata_value(extension)
            ));
        }
    }
    fs::write(&handler_path, metadata.as_bytes()).with_context(|| {
        format!(
            "failed writing gui handler metadata: {}",
            handler_path.display()
        )
    })?;

    Ok(projected)
}

pub fn remove_exposed_gui_asset(layout: &PrefixLayout, asset: &GuiExposureAsset) -> Result<()> {
    let path = gui_asset_path(layout, &asset.rel_path)?;
    if !path.exists() {
        return Ok(());
    }
    fs::remove_file(&path)
        .with_context(|| format!("failed to remove exposed gui asset: {}", path.display()))?;
    prune_empty_gui_dirs(layout, path.parent())?;
    Ok(())
}

#[cfg(windows)]
pub(crate) fn render_gui_launcher(app: &ArtifactGuiApp, source_path: &Path) -> String {
    format!(
        "@echo off\r\nREM {}\r\n\"{}\" %*\r\n",
        sanitize_gui_metadata_value(&app.display_name),
        source_path.display()
    )
}

#[cfg(target_os = "linux")]
pub(crate) fn render_gui_launcher(app: &ArtifactGuiApp, source_path: &Path) -> String {
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

#[cfg(all(not(windows), not(target_os = "linux")))]
pub(crate) fn render_gui_launcher(app: &ArtifactGuiApp, source_path: &Path) -> String {
    #[cfg(target_os = "macos")]
    {
        if source_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("app"))
            .unwrap_or(false)
        {
            return format!(
                "#!/bin/sh\n# {}\nopen -a \"{}\" --args \"$@\"\n",
                sanitize_gui_metadata_value(&app.display_name),
                source_path.display()
            );
        }
    }

    format!(
        "#!/bin/sh\n# {}\nexec \"{}\" \"$@\"\n",
        sanitize_gui_metadata_value(&app.display_name),
        source_path.display()
    )
}

fn gui_launcher_extension() -> &'static str {
    if cfg!(windows) {
        "cmd"
    } else if cfg!(target_os = "linux") {
        "desktop"
    } else {
        "command"
    }
}

pub(crate) fn normalize_gui_token(value: &str) -> String {
    let mut normalized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if normalized.is_empty() {
        normalized.push('_');
    }
    normalized
}

pub(crate) fn normalized_protocol_scheme(scheme: &str) -> Result<String> {
    let trimmed = scheme.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Err(anyhow!("protocol scheme must not be empty"));
    }
    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else {
        return Err(anyhow!("protocol scheme must not be empty"));
    };
    if !first.is_ascii_alphabetic() {
        return Err(anyhow!("protocol scheme must start with an ASCII letter"));
    }
    if chars.any(|ch| !(ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.')) {
        return Err(anyhow!("protocol scheme contains invalid character(s)"));
    }
    Ok(trimmed)
}

pub(crate) fn normalized_extension(extension: &str) -> Result<String> {
    let trimmed = extension.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Err(anyhow!("file association extension must not be empty"));
    }
    let normalized = if trimmed.starts_with('.') {
        trimmed
    } else {
        format!(".{trimmed}")
    };
    if normalized
        .chars()
        .skip(1)
        .any(|ch| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'))
    {
        return Err(anyhow!(
            "file association extension contains invalid character(s)"
        ));
    }
    Ok(normalized)
}

pub(crate) fn sanitize_gui_metadata_value(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(target_os = "linux")]
pub(crate) fn sanitize_desktop_list_token(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch == '\n' || ch == '\r' || ch == ';' {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn sanitize_desktop_list_token(value: &str) -> String {
    sanitize_gui_metadata_value(value)
}

fn validated_relative_gui_storage_path(path: &str) -> Result<&Path> {
    let relative = Path::new(path);
    if relative.is_absolute() {
        return Err(anyhow!("gui storage path must be relative: {}", path));
    }
    if relative.as_os_str().is_empty() {
        return Err(anyhow!("gui storage path must not be empty"));
    }
    if relative
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(anyhow!("gui storage path must not include '..': {}", path));
    }
    Ok(relative)
}

fn prune_empty_gui_dirs(layout: &PrefixLayout, start: Option<&Path>) -> Result<()> {
    let mut current = start.map(PathBuf::from);
    let gui_root = layout.gui_dir();
    while let Some(dir) = current {
        if !dir.starts_with(&gui_root) || dir == gui_root {
            break;
        }

        let mut entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                current = dir.parent().map(PathBuf::from);
                continue;
            }
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed reading gui dir: {}", dir.display()));
            }
        };

        if entries.next().is_some() {
            break;
        }

        fs::remove_dir(&dir)
            .with_context(|| format!("failed pruning gui dir: {}", dir.display()))?;
        current = dir.parent().map(PathBuf::from);
    }
    Ok(())
}

fn prune_empty_completion_dirs(layout: &PrefixLayout, start: Option<&Path>) -> Result<()> {
    let mut current = start.map(PathBuf::from);
    let completions_root = layout.completions_dir();
    while let Some(dir) = current {
        if !dir.starts_with(&completions_root) || dir == completions_root {
            break;
        }

        let mut entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                current = dir.parent().map(PathBuf::from);
                continue;
            }
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed reading completion dir: {}", dir.display()));
            }
        };
        if entries.next().is_some() {
            break;
        }

        fs::remove_dir(&dir)
            .with_context(|| format!("failed pruning completion dir: {}", dir.display()))?;
        current = dir.parent().map(PathBuf::from);
    }
    Ok(())
}

fn normalize_completion_token(value: &str) -> String {
    let mut normalized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if normalized.is_empty() {
        normalized.push('_');
    }
    normalized
}

fn normalize_completion_source_path(path: &Path) -> String {
    let mut parts = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(normalize_completion_token(&value.to_string_lossy())),
            _ => None,
        })
        .collect::<Vec<_>>();
    if parts.is_empty() {
        parts.push("_".to_string());
    }
    parts.join("--")
}

fn validated_relative_completion_source_path(path: &str) -> Result<&Path> {
    let relative = Path::new(path);
    if relative.is_absolute() {
        return Err(anyhow!("completion path must be relative: {}", path));
    }
    if relative.as_os_str().is_empty() {
        return Err(anyhow!("completion path must not be empty"));
    }
    if relative
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(anyhow!("completion path must not include '..': {}", path));
    }
    Ok(relative)
}

fn validated_relative_completion_storage_path(path: &str) -> Result<&Path> {
    let relative = Path::new(path);
    if relative.is_absolute() {
        return Err(anyhow!(
            "completion storage path must be relative: {}",
            path
        ));
    }
    if relative.as_os_str().is_empty() {
        return Err(anyhow!("completion storage path must not be empty"));
    }
    if relative
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(anyhow!(
            "completion storage path must not include '..': {}",
            path
        ));
    }
    Ok(relative)
}

pub(crate) fn validated_relative_binary_path(path: &str) -> Result<&Path> {
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

fn resolve_binary_source_path(install_root: &Path, binary_rel_path: &str) -> Result<PathBuf> {
    let source_rel = validated_relative_binary_path(binary_rel_path)?;
    let source_path = install_root.join(source_rel);
    if source_path.exists() {
        return Ok(source_path);
    }

    if let Some(stripped_rel) = stripped_macos_bundle_exec_rel_path(source_rel) {
        let stripped_source_path = install_root.join(stripped_rel);
        if stripped_source_path.exists() {
            return Ok(stripped_source_path);
        }
    }

    Err(anyhow!(
        "declared binary path '{}' was not found in install root: {}",
        binary_rel_path,
        source_path.display()
    ))
}

fn stripped_macos_bundle_exec_rel_path(path: &Path) -> Option<PathBuf> {
    let components = path.components().collect::<Vec<_>>();
    if components.len() < 4 {
        return None;
    }

    let Component::Normal(bundle_root) = components.first()? else {
        return None;
    };
    if !bundle_root
        .to_string_lossy()
        .to_ascii_lowercase()
        .ends_with(".app")
    {
        return None;
    }
    if components.get(1)?.as_os_str() != "Contents" {
        return None;
    }
    if components.get(2)?.as_os_str() != "MacOS" {
        return None;
    }
    if components
        .iter()
        .skip(1)
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }

    let mut stripped = PathBuf::new();
    for component in components.iter().skip(1) {
        stripped.push(component.as_os_str());
    }
    Some(stripped)
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
