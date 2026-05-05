use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use crate::types::{
    HostPlatform, IntegrationActivationPlan, IntegrationActivationScope, IntegrationAdapterKind,
    IntegrationDesiredState, IntegrationProjection, IntegrationReasonCode,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostActivationContext {
    pub platform: HostPlatform,
    pub env: BTreeMap<String, String>,
    pub home: Option<String>,
    pub user_profile: Option<String>,
    pub symlink_supported: bool,
    pub windows_user_services_supported: bool,
    pub service_requires_admin: bool,
    pub prefix: String,
}

impl HostActivationContext {
    pub fn linux() -> Self {
        Self::new(HostPlatform::Linux, "/prefix")
    }

    pub fn macos() -> Self {
        Self::new(HostPlatform::Macos, "/prefix")
    }

    pub fn windows() -> Self {
        Self::new(HostPlatform::Windows, "C:\\Crosspack")
    }

    fn new(platform: HostPlatform, prefix: &str) -> Self {
        Self {
            platform,
            env: BTreeMap::new(),
            home: None,
            user_profile: None,
            symlink_supported: true,
            windows_user_services_supported: false,
            service_requires_admin: false,
            prefix: prefix.to_string(),
        }
    }

    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.insert(key.to_string(), value.to_string());
        self
    }

    pub fn with_home(mut self, home: &str) -> Self {
        self.home = Some(home.to_string());
        self
    }

    pub fn with_user_profile(mut self, user_profile: &str) -> Self {
        self.user_profile = Some(user_profile.to_string());
        self
    }

    pub fn with_symlink_support(mut self, supported: bool) -> Self {
        self.symlink_supported = supported;
        self
    }

    pub fn with_prefix(mut self, prefix: &str) -> Self {
        self.prefix = prefix.to_string();
        self
    }

    pub fn with_windows_user_services_supported(mut self, supported: bool) -> Self {
        self.windows_user_services_supported = supported;
        self
    }

    pub fn with_service_requires_admin(mut self, requires_admin: bool) -> Self {
        self.service_requires_admin = requires_admin;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceActivationMetadata {
    pub name: String,
    pub source: Option<String>,
    pub macos_launch_agent: Option<String>,
    pub windows_service: Option<String>,
}

impl ServiceActivationMetadata {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            source: None,
            macos_launch_agent: None,
            windows_service: None,
        }
    }

    pub fn with_source(mut self, source: &str) -> Self {
        self.source = Some(source.to_string());
        self
    }

    pub fn with_macos_launch_agent(mut self, source: &str) -> Self {
        self.macos_launch_agent = Some(source.to_string());
        self
    }

    pub fn with_windows_service(mut self, source: &str) -> Self {
        self.windows_service = Some(source.to_string());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationPlanError {
    pub reason_code: IntegrationReasonCode,
}

impl IntegrationPlanError {
    fn new(reason_code: IntegrationReasonCode) -> Self {
        Self { reason_code }
    }
}

impl fmt::Display for IntegrationPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "integration planning failed: {}",
            self.reason_code.as_str()
        )
    }
}

impl Error for IntegrationPlanError {}

pub fn plan_docker_cli_plugin_activation(
    host: &HostActivationContext,
    package: &str,
    projection: &IntegrationProjection,
) -> Result<IntegrationActivationPlan, IntegrationPlanError> {
    validate_host_root(host.platform, &host.prefix)?;
    validate_host_leaf(package)?;
    validate_relative_source_path(&projection.rel_path)?;
    let docker_config = if let Some(config) = host.env.get("DOCKER_CONFIG") {
        validate_host_root(host.platform, config)?;
        config.clone()
    } else {
        match host.platform {
            HostPlatform::Linux | HostPlatform::Macos => {
                let home = host.home.as_deref().ok_or_else(|| {
                    IntegrationPlanError::new(IntegrationReasonCode::UnsupportedHost)
                })?;
                validate_host_root(host.platform, home)?;
                join_native(host.platform, home, &[".docker"])
            }
            HostPlatform::Windows => {
                let user_profile = host.user_profile.as_deref().ok_or_else(|| {
                    IntegrationPlanError::new(IntegrationReasonCode::UnsupportedHost)
                })?;
                validate_host_root(host.platform, user_profile)?;
                join_native(host.platform, user_profile, &[".docker"])
            }
        }
    };

    Ok(IntegrationActivationPlan {
        package_state_key: package.to_string(),
        package: package.to_string(),
        integration_key: projection.key.clone(),
        kind: projection.kind.clone(),
        adapter: IntegrationAdapterKind::DockerCli,
        scope: IntegrationActivationScope::None,
        desired_state: IntegrationDesiredState::Enabled,
        host_path: join_native(
            host.platform,
            &docker_config,
            &["cli-plugins", file_name(&projection.rel_path)],
        ),
        source_path: integration_source_path(host, &projection.rel_path),
    })
}

pub fn plan_path_plugin_activation(
    host: &HostActivationContext,
    package: &str,
    host_name: &str,
    projection: &IntegrationProjection,
) -> Result<IntegrationActivationPlan, IntegrationPlanError> {
    validate_host_root(host.platform, &host.prefix)?;
    validate_host_leaf(package)?;
    validate_host_leaf(host_name)?;
    let expected_host_name = expected_path_plugin_host_name(&projection.key)?;
    if host_name != expected_host_name {
        return Err(IntegrationPlanError::new(
            IntegrationReasonCode::InvalidServiceMetadata,
        ));
    }
    validate_relative_source_path(&projection.rel_path)?;
    Ok(IntegrationActivationPlan {
        package_state_key: package.to_string(),
        package: package.to_string(),
        integration_key: projection.key.clone(),
        kind: projection.kind.clone(),
        adapter: IntegrationAdapterKind::PathPluginBin,
        scope: IntegrationActivationScope::None,
        desired_state: IntegrationDesiredState::Enabled,
        host_path: join_native(
            host.platform,
            &host.prefix,
            &[
                "bin",
                path_plugin_host_file_name(host.platform, host_name).as_str(),
            ],
        ),
        source_path: integration_source_path(host, &projection.rel_path),
    })
}

pub fn plan_service_activation(
    host: &HostActivationContext,
    package: &str,
    metadata: &ServiceActivationMetadata,
) -> Result<IntegrationActivationPlan, IntegrationPlanError> {
    validate_host_root(host.platform, &host.prefix)?;
    validate_host_leaf(package)?;
    validate_host_leaf(&metadata.name)?;
    let integration_key = format!("service:{}", metadata.name);
    match host.platform {
        HostPlatform::Linux => {
            let source = metadata
                .source
                .as_deref()
                .filter(|path| path.ends_with(".service"));
            let source = source.ok_or_else(|| {
                IntegrationPlanError::new(IntegrationReasonCode::InvalidServiceMetadata)
            })?;
            validate_relative_source_path(source)?;
            Ok(IntegrationActivationPlan {
                package_state_key: package.to_string(),
                package: package.to_string(),
                integration_key,
                kind: "service".to_string(),
                adapter: IntegrationAdapterKind::SystemdUser,
                scope: IntegrationActivationScope::User,
                desired_state: IntegrationDesiredState::Running,
                host_path: format!("systemd-user:{}", file_name(source)),
                source_path: integration_source_path(host, source),
            })
        }
        HostPlatform::Macos => {
            let source = metadata
                .macos_launch_agent
                .as_deref()
                .filter(|path| path.ends_with(".plist"));
            let source = source.ok_or_else(|| {
                IntegrationPlanError::new(IntegrationReasonCode::InvalidServiceMetadata)
            })?;
            validate_relative_source_path(source)?;
            let home = host
                .home
                .as_deref()
                .ok_or_else(|| IntegrationPlanError::new(IntegrationReasonCode::UnsupportedHost))?;
            validate_host_root(host.platform, home)?;
            Ok(IntegrationActivationPlan {
                package_state_key: package.to_string(),
                package: package.to_string(),
                integration_key,
                kind: "service".to_string(),
                adapter: IntegrationAdapterKind::LaunchdUser,
                scope: IntegrationActivationScope::User,
                desired_state: IntegrationDesiredState::Running,
                host_path: join_native(
                    host.platform,
                    home,
                    &["Library", "LaunchAgents", file_name(source)],
                ),
                source_path: integration_source_path(host, source),
            })
        }
        HostPlatform::Windows => {
            let source = metadata.windows_service.as_deref().ok_or_else(|| {
                IntegrationPlanError::new(IntegrationReasonCode::InvalidServiceMetadata)
            })?;
            validate_relative_source_path(source)?;
            if !host.windows_user_services_supported || host.service_requires_admin {
                return Err(IntegrationPlanError::new(
                    IntegrationReasonCode::EscalationRequired,
                ));
            }
            Ok(IntegrationActivationPlan {
                package_state_key: package.to_string(),
                package: package.to_string(),
                integration_key,
                kind: "service".to_string(),
                adapter: IntegrationAdapterKind::WindowsServiceUser,
                scope: IntegrationActivationScope::User,
                desired_state: IntegrationDesiredState::Running,
                host_path: format!("windows-service-user:{}", metadata.name),
                source_path: integration_source_path(host, source),
            })
        }
    }
}

fn integration_source_path(host: &HostActivationContext, rel_path: &str) -> String {
    let parts: Vec<&str> = rel_path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    join_native(host.platform, &host.prefix, &["share", "integrations"])
        + separator(host.platform)
        + &parts.join(separator(host.platform))
}

fn validate_relative_source_path(path: &str) -> Result<(), IntegrationPlanError> {
    if path.is_empty()
        || path == "."
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || path.chars().any(char::is_control)
        || looks_like_windows_drive(path)
    {
        return Err(IntegrationPlanError::new(
            IntegrationReasonCode::InvalidServiceMetadata,
        ));
    }
    Ok(())
}

fn validate_host_leaf(value: &str) -> Result<(), IntegrationPlanError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || looks_like_windows_drive(value)
    {
        return Err(IntegrationPlanError::new(
            IntegrationReasonCode::InvalidServiceMetadata,
        ));
    }
    Ok(())
}

fn validate_host_root(platform: HostPlatform, path: &str) -> Result<(), IntegrationPlanError> {
    if !is_absolute_for_platform(platform, path)
        || (platform == HostPlatform::Windows && is_windows_unc_path(path))
    {
        return Err(IntegrationPlanError::new(
            IntegrationReasonCode::UnsupportedHost,
        ));
    }
    Ok(())
}

fn is_windows_unc_path(path: &str) -> bool {
    path.starts_with("\\\\") || path.starts_with("//")
}

fn looks_like_windows_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

fn join_native(platform: HostPlatform, base: &str, parts: &[&str]) -> String {
    let sep = separator(platform);
    let mut joined = normalize_base_path(platform, base)
        .trim_end_matches(['/', '\\'])
        .to_string();
    for part in parts {
        joined.push_str(sep);
        joined.push_str(part.trim_matches(['/', '\\']));
    }
    joined
}

fn normalize_base_path(platform: HostPlatform, base: &str) -> String {
    match platform {
        HostPlatform::Linux | HostPlatform::Macos => base.to_string(),
        HostPlatform::Windows => base.replace('/', "\\"),
    }
}

fn separator(platform: HostPlatform) -> &'static str {
    match platform {
        HostPlatform::Linux | HostPlatform::Macos => "/",
        HostPlatform::Windows => "\\",
    }
}

fn path_plugin_host_file_name(platform: HostPlatform, host_name: &str) -> String {
    match platform {
        HostPlatform::Linux | HostPlatform::Macos => host_name.to_string(),
        HostPlatform::Windows => format!("{host_name}.cmd"),
    }
}

fn expected_path_plugin_host_name(key: &str) -> Result<String, IntegrationPlanError> {
    let mut parts = key.split(':');
    let kind = parts.next();
    let host = parts.next();
    let name = parts.next();
    if kind != Some("path_plugin") || host.is_none() || name.is_none() || parts.next().is_some() {
        return Err(IntegrationPlanError::new(
            IntegrationReasonCode::InvalidServiceMetadata,
        ));
    }
    let host = host.unwrap();
    let name = name.unwrap();
    validate_host_leaf(host)?;
    validate_host_leaf(name)?;
    Ok(format!("{host}-{name}"))
}

fn is_absolute_for_platform(platform: HostPlatform, path: &str) -> bool {
    match platform {
        HostPlatform::Linux | HostPlatform::Macos => path.starts_with('/'),
        HostPlatform::Windows => {
            let bytes = path.as_bytes();
            path.starts_with("\\\\")
                || (bytes.len() >= 3
                    && bytes[1] == b':'
                    && (bytes[2] == b'\\' || bytes[2] == b'/')
                    && bytes[0].is_ascii_alphabetic())
        }
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}
