# Cross-Platform Host Activation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement fully typed host activation for Docker CLI plugins, PATH plugins, and services across Linux, macOS, and Windows without arbitrary scripts.

**Architecture:** Keep prefix projection separate from host activation. Add an installer-owned activation state model, a pure planner over fake host capabilities, and typed adapters that are tested through fake filesystems/executors before any install-time service activation is wired. CLI commands orchestrate read/apply flows but do not own host mutation policy.

**Tech Stack:** Rust workspace, `crosspack-core` manifest schemas, `crosspack-installer` prefix/state/transaction APIs, `crosspack-cli` command routing/output contracts, fake executor/filesystem tests.

---

## Reference Spec

- `.agents/specs/typed-host-integrations-expansion-spec.md`
- Current completed slice: `crosspack integrations list/status` over prefix projection state.

## File Map

- Modify: `crates/crosspack-installer/src/types.rs`
  - Add host activation structs/enums: platform, adapter kind, scope, desired/applied state, reason code, operation plan, rollback entry.
- Create: `crates/crosspack-installer/src/activation_state.rs`
  - Read/write/parse activation sidecars.
- Create: `crates/crosspack-installer/src/activation_plan.rs`
  - Pure planner for Docker/PATH/service activation and fake host capability inputs.
- Create: `crates/crosspack-installer/src/activation_adapters.rs`
  - Adapter traits and fake filesystem/executor friendly implementations.
- Modify: `crates/crosspack-installer/src/native.rs`
  - Reuse or route service command execution behind the new adapter abstraction.
- Modify: `crates/crosspack-installer/src/layout.rs`
  - Add activation state paths and any Crosspack-owned activation directories.
- Modify: `crates/crosspack-installer/src/lib.rs`
  - Export new activation APIs.
- Modify: `crates/crosspack-installer/src/uninstall.rs`
  - Remove host activation before prefix projection.
- Modify: `crates/crosspack-cli/src/main.rs`
  - Add `integrations enable/disable` command variants if not already present.
- Modify: `crates/crosspack-cli/src/dispatch.rs`
  - Route integration commands.
- Modify: `crates/crosspack-cli/src/command_flows.rs`
  - Implement CLI orchestration and output formatting only.
- Modify: `crates/crosspack-cli/src/core_flows.rs`
  - Wire service `enable = true` activation into install transaction after all adapters are tested.
- Modify: `crates/crosspack-cli/src/tests.rs`
  - CLI contract tests for list/status/enable/disable and install-time activation.
- Modify: `crates/crosspack-installer/src/tests.rs`
  - State, planner, adapter, rollback, and uninstall tests.
- Optionally modify: `crates/crosspack-core/src/manifest.rs`
  - Future platform-specific service metadata fields. Do not add until tasks explicitly require them.

---

## Task 1: Activation State Model

**Files:**
- Modify: `crates/crosspack-installer/src/types.rs`
- Create: `crates/crosspack-installer/src/activation_state.rs`
- Modify: `crates/crosspack-installer/src/layout.rs`
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-installer/src/tests.rs`

- [ ] **Step 1: Write failing round-trip test**

Add a test that writes two activation records and reads them back:

```rust
#[test]
fn activation_state_round_trips_multiple_platform_records() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let records = vec![
        IntegrationActivationRecord {
            package_state_key: "default__linux__core__docker-compose".to_string(),
            package: "docker-compose".to_string(),
            integration_key: "docker_cli_plugin:compose".to_string(),
            kind: "docker_cli_plugin".to_string(),
            adapter: IntegrationAdapterKind::DockerCli,
            scope: IntegrationActivationScope::None,
            desired_state: IntegrationDesiredState::Enabled,
            applied_state: IntegrationAppliedState::Enabled,
            host_path: Some("/home/test/.docker/cli-plugins/docker-compose".to_string()),
            reason_code: IntegrationReasonCode::Ok,
        },
        IntegrationActivationRecord {
            package_state_key: "default__macos__core__caddy".to_string(),
            package: "caddy".to_string(),
            integration_key: "service:caddy".to_string(),
            kind: "service".to_string(),
            adapter: IntegrationAdapterKind::LaunchdUser,
            scope: IntegrationActivationScope::User,
            desired_state: IntegrationDesiredState::Running,
            applied_state: IntegrationAppliedState::Unsupported,
            host_path: Some("/Users/test/Library/LaunchAgents/com.example.caddy.plist".to_string()),
            reason_code: IntegrationReasonCode::InvalidServiceMetadata,
        },
    ];

    write_integration_activation_state(&layout, &records).expect("must write activation state");

    assert_eq!(
        read_integration_activation_state(&layout).expect("must read activation state"),
        records
    );
}
```

- [ ] **Step 2: Run test to verify red**

Run: `cargo test -p crosspack-installer activation_state_round_trips_multiple_platform_records`

Expected: FAIL because activation types/functions do not exist.

- [ ] **Step 3: Implement minimal activation types**

Add enums to `types.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationAdapterKind {
    None,
    DockerCli,
    PathPluginBin,
    SystemdUser,
    LaunchdUser,
    WindowsServiceUser,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationActivationScope {
    None,
    User,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationDesiredState {
    Projected,
    Enabled,
    Running,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationAppliedState {
    Projected,
    Installed,
    Enabled,
    Running,
    Stopped,
    Failed,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationReasonCode {
    Ok,
    NotEnabled,
    UnsupportedHost,
    AdapterToolMissing,
    HostPathConflict,
    EscalationRequired,
    NativeCommandFailed,
    InvalidServiceMetadata,
    StateMissing,
    StateAmbiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationActivationRecord {
    pub package_state_key: String,
    pub package: String,
    pub integration_key: String,
    pub kind: String,
    pub adapter: IntegrationAdapterKind,
    pub scope: IntegrationActivationScope,
    pub desired_state: IntegrationDesiredState,
    pub applied_state: IntegrationAppliedState,
    pub host_path: Option<String>,
    pub reason_code: IntegrationReasonCode,
}
```

Implement `as_str()`/`from_str()` methods for every enum using the exact spec strings.

- [ ] **Step 4: Implement state file path and parser**

Add to `PrefixLayout`:

```rust
pub fn integration_activation_state_path(&self) -> PathBuf {
    self.installed_state_dir().join("integrations.activation")
}
```

Create `activation_state.rs` with versioned tab-separated rows:

```rust
const INTEGRATION_ACTIVATION_STATE_VERSION: u32 = 1;
```

Rows must reject tabs/newlines in fields and preserve native path spelling.

- [ ] **Step 5: Run test to verify green**

Run: `cargo test -p crosspack-installer activation_state_round_trips_multiple_platform_records`

Expected: PASS.

- [ ] **Step 6: Add malformed state tests**

Test unsupported version, invalid enum values, missing columns, and tab/newline rejection.

Run: `cargo test -p crosspack-installer activation_state_`

Expected: PASS.

---

## Task 2: Pure Cross-Platform Planner

**Files:**
- Create: `crates/crosspack-installer/src/activation_plan.rs`
- Modify: `crates/crosspack-installer/src/types.rs`
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-installer/src/tests.rs`

- [ ] **Step 1: Write failing Docker path tests**

Add tests for Linux, macOS, Windows, relative `DOCKER_CONFIG`, and missing home/profile:

```rust
#[test]
fn docker_cli_plugin_plan_uses_platform_docker_config_precedence() {
    let projection = IntegrationProjection {
        kind: "docker_cli_plugin".to_string(),
        key: "docker_cli_plugin:compose".to_string(),
        rel_path: "docker/cli-plugins/docker-compose".to_string(),
    };

    let linux = FakeHostContext::linux()
        .with_home("/home/test")
        .with_env("DOCKER_CONFIG", "/tmp/docker-config");
    assert_eq!(
        plan_docker_cli_plugin_activation(&linux, "docker-compose", &projection)
            .expect("must plan linux docker plugin")
            .host_path,
        "/tmp/docker-config/cli-plugins/docker-compose"
    );

    let macos = FakeHostContext::macos().with_home("/Users/test");
    assert_eq!(
        plan_docker_cli_plugin_activation(&macos, "docker-compose", &projection)
            .expect("must plan macos docker plugin")
            .host_path,
        "/Users/test/.docker/cli-plugins/docker-compose"
    );

    let windows = FakeHostContext::windows().with_user_profile("C:\\Users\\test");
    assert_eq!(
        plan_docker_cli_plugin_activation(&windows, "docker-compose", &projection)
            .expect("must plan windows docker plugin")
            .host_path,
        "C:\\Users\\test\\.docker\\cli-plugins\\docker-compose"
    );
}
```

- [ ] **Step 2: Run test to verify red**

Run: `cargo test -p crosspack-installer docker_cli_plugin_plan_uses_platform_docker_config_precedence`

Expected: FAIL because planner does not exist.

- [ ] **Step 3: Implement fake host context and Docker planner**

Create pure structs:

```rust
pub enum HostPlatform { Linux, Macos, Windows }

pub struct FakeHostContext {
    pub platform: HostPlatform,
    pub env: BTreeMap<String, String>,
    pub home: Option<String>,
    pub user_profile: Option<String>,
    pub symlink_supported: bool,
}

pub struct IntegrationActivationPlan {
    pub package: String,
    pub integration_key: String,
    pub kind: String,
    pub adapter: IntegrationAdapterKind,
    pub scope: IntegrationActivationScope,
    pub desired_state: IntegrationDesiredState,
    pub host_path: String,
    pub source_path: String,
}
```

Planner returns `IntegrationReasonCode::UnsupportedHost` for relative `DOCKER_CONFIG` or missing home/profile.

- [ ] **Step 4: Add PATH plugin and service planner tests**

Tests must cover:

- PATH plugin plans Crosspack-owned bin exposure on Linux/macOS.
- PATH plugin plans Windows shim/exposure path with `\\` spelling.
- Linux service with `.service` source plans `systemd-user`.
- macOS service with `macos_launch_agent` plist source plans `launchd-user`.
- Windows service with `windows_service` descriptor plans `windows-service-user` when user-scope activation is supported without admin.
- macOS service without plist returns `invalid-service-metadata`.
- Windows service without Windows descriptor returns `invalid-service-metadata`.

Run: `cargo test -p crosspack-installer activation_plan_`

Expected: PASS after implementation.

---

## Task 3: Docker CLI Plugin Symlink Adapter

**Files:**
- Create/modify: `crates/crosspack-installer/src/activation_adapters.rs`
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-installer/src/tests.rs`

- [ ] **Step 1: Write failing fake filesystem tests**

Add tests:

```rust
#[test]
fn docker_adapter_creates_idempotent_owned_symlink_and_rejects_conflicts() {
    let mut fs = FakeActivationFs::new(HostPlatform::Linux).with_symlink_support(true);
    fs.write_file("/prefix/share/integrations/docker/cli-plugins/docker-compose", b"plugin");

    let plan = IntegrationActivationPlan {
        package: "docker-compose".to_string(),
        integration_key: "docker_cli_plugin:compose".to_string(),
        kind: "docker_cli_plugin".to_string(),
        adapter: IntegrationAdapterKind::DockerCli,
        scope: IntegrationActivationScope::None,
        desired_state: IntegrationDesiredState::Enabled,
        host_path: "/home/test/.docker/cli-plugins/docker-compose".to_string(),
        source_path: "/prefix/share/integrations/docker/cli-plugins/docker-compose".to_string(),
    };

    assert_eq!(apply_docker_cli_plugin_plan(&mut fs, &plan).reason_code, IntegrationReasonCode::Ok);
    assert_eq!(apply_docker_cli_plugin_plan(&mut fs, &plan).reason_code, IntegrationReasonCode::Ok);

    fs.write_file("/home/test/.docker/cli-plugins/docker-buildx", b"foreign");
    let mut conflicting = plan.clone();
    conflicting.host_path = "/home/test/.docker/cli-plugins/docker-buildx".to_string();
    assert_eq!(
        apply_docker_cli_plugin_plan(&mut fs, &conflicting).reason_code,
        IntegrationReasonCode::HostPathConflict
    );
}
```

- [ ] **Step 2: Run test to verify red**

Run: `cargo test -p crosspack-installer docker_adapter_`

Expected: FAIL because fake fs/adapter does not exist.

- [ ] **Step 3: Implement fake filesystem and adapter**

Implement in-memory fake file entries: file, symlink, directory. Adapter behavior:

- Create parent directories.
- Create symlink only.
- Reject non-owned existing file.
- Treat same target symlink as idempotent success.
- Return `EscalationRequired` when platform is Windows and `symlink_supported=false`.

- [ ] **Step 4: Add Windows and macOS tests**

Additional tests must cover:

- `disable_docker_cli_plugin_plan_removes_owned_symlink_on_linux_macos_windows`
- `disable_docker_cli_plugin_plan_leaves_foreign_file_and_returns_host_path_conflict`
- `disable_docker_cli_plugin_plan_is_idempotent_when_destination_missing`
- `apply_docker_cli_plugin_plan_replaces_previous_owned_symlink_target_with_rollback_record`

Run: `cargo test -p crosspack-installer docker_adapter_`

Expected: PASS for Linux/macOS/Windows fake hosts.

---

## Task 4: PATH Plugin Adapter

**Files:**
- Modify: `crates/crosspack-installer/src/activation_adapters.rs`
- Test: `crates/crosspack-installer/src/tests.rs`

- [ ] **Step 1: Write failing PATH adapter tests**

Tests must prove:

- Linux/macOS create symlink under Crosspack-owned bin path.
- Windows creates shim/exposure using existing executable exposure semantics or fake equivalent.
- Existing foreign file conflicts.
- Existing owned exposure is idempotent.
- Disable removes owned symlink/shim on Linux, macOS, and Windows fake hosts.
- Disable is idempotent when the owned exposure is already absent.
- Disable refuses to remove foreign files.

Run: `cargo test -p crosspack-installer path_plugin_adapter_`

Expected: FAIL before implementation.

- [ ] **Step 2: Implement minimal adapter**

Rules:

- Destination must be under Crosspack prefix.
- Unix-like fake hosts use symlink.
- Windows fake host uses shim/exposure record, not arbitrary host directory mutation.
- Persist activation state only after success.
- Disable clears activation state only after owned exposure removal succeeds.

- [ ] **Step 3: Run tests**

Run: `cargo test -p crosspack-installer path_plugin_adapter_`

Expected: PASS.

---

## Task 5: Manifest Metadata Expansion For macOS/Windows Services

**Files:**
- Modify: `crates/crosspack-core/src/manifest.rs`
- Test: core manifest tests

- [ ] **Step 1: Write failing manifest tests**

Tests:

- Service integration accepts `linux_systemd_user`, `macos_launch_agent`, and `windows_service` fields.
- `source` remains accepted as Linux compatibility alias.
- Docker/PATH still reject `enable`.
- Unsafe platform-specific source paths are rejected.

Run: `cargo test -p crosspack-core integration_service_platform_sources_`

Expected: FAIL before schema expansion.

- [ ] **Step 2: Implement schema expansion**

Add fields only to `PackageIntegration::Service`:

```rust
linux_systemd_user: Option<String>,
macos_launch_agent: Option<String>,
windows_service: Option<String>,
```

Keep `source` compatibility if it already exists; map it to Linux systemd user semantics.

- [ ] **Step 3: Run core tests**

Run: `cargo test -p crosspack-core integration_service_platform_sources_`

Expected: PASS.

---

## Task 6: Service Adapter Abstraction And Fake Executors

**Files:**
- Modify: `crates/crosspack-installer/src/activation_adapters.rs`
- Modify: `crates/crosspack-installer/src/native.rs`
- Test: `crates/crosspack-installer/src/tests.rs`

- [ ] **Step 1: Write failing command-sequence tests**

Tests must cover:

- Linux systemd user install/enable/start/status sequence.
- macOS launchd user bootstrap/enable/kickstart/print sequence.
- Windows user-service sequence when no admin is required.
- Windows SCM/admin-required path returns `escalation-required` without mutation.
- Linux, macOS, and Windows stop/disable/remove command sequences.
- Linux, macOS, and Windows status parsing into `running`, `stopped`, `failed`, and `unsupported`.

Run: `cargo test -p crosspack-installer service_adapter_`

Expected: FAIL before abstraction exists.

- [ ] **Step 2: Implement executor trait**

```rust
pub trait ActivationCommandExecutor {
    fn run(&mut self, program: &str, args: &[String]) -> NativeCommandResult;
}

pub struct NativeCommandResult {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}
```

Do not execute commands in tests. Use fake executor logs.

- [ ] **Step 3: Implement service adapters**

Implement only typed command construction and output parsing. Keep real command execution behind the trait.

- [ ] **Step 4: Run service adapter tests**

Run: `cargo test -p crosspack-installer service_adapter_`

Expected: PASS.

---

## Task 7: CLI Enable/Disable And Service Status Wiring

**Files:**
- Modify: `crates/crosspack-cli/src/main.rs`
- Modify: `crates/crosspack-cli/src/dispatch.rs`
- Modify: `crates/crosspack-cli/src/command_flows.rs`
- Test: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Write failing CLI parse/output tests**

Tests:

- `crosspack integrations enable docker-compose compose` parses.
- `crosspack integrations disable docker-compose docker_cli_plugin:compose` parses.
- Enable output includes `state=enabled adapter=docker-cli reason=ok`.
- Disable output includes `state=projected adapter=docker-cli reason=ok`.
- Ambiguous short names require full keys.
- Windows path output with `\\` stays parseable.
- `crosspack services list` includes declared services and activation state summaries.
- `crosspack services status caddy caddy` includes activation state fields: `adapter=<adapter> scope=user applied=<bool> reason=<reason>`.
- `crosspack services start caddy caddy`, `crosspack services stop caddy caddy`, and `crosspack services restart caddy caddy` update and print activation state consistently.
- Linux systemd, macOS launchd, and Windows failure outputs use identical key ordering.

Run: `cargo test -p crosspack-cli integrations_`

Expected: FAIL before command variants exist.

- [ ] **Step 2: Implement CLI orchestration**

Rules:

- CLI resolves package/integration and calls installer APIs.
- CLI does not inspect host filesystem directly.
- Plain output key ordering follows spec.
- Existing service commands read/write the new activation state where applicable.

- [ ] **Step 3: Run CLI tests**

Run: `cargo test -p crosspack-cli integrations_`

Expected: PASS.

---

## Task 8: Transaction, Rollback, And Uninstall Integration

**Files:**
- Modify: `crates/crosspack-cli/src/core_flows.rs`
- Modify: `crates/crosspack-cli/src/command_flows.rs`
- Modify: `crates/crosspack-installer/src/uninstall.rs`
- Modify: `crates/crosspack-installer/src/transactions.rs` if rollback payload APIs need extension
- Test: `crates/crosspack-cli/src/tests.rs`
- Test: `crates/crosspack-installer/src/tests.rs`

- [ ] **Step 1: Write failing cross-platform transaction tests**

Tests:

- Linux fake host install with `service.enable=true` activates after prefix projection.
- macOS fake host install with LaunchAgent metadata activates.
- macOS fake host with only Linux unit fails `invalid-service-metadata` and rolls back.
- Windows fake host activates only with Windows service metadata and no admin requirement.
- Windows fake host requiring admin fails `escalation-required` and rolls back.
- Uninstall removes activation before prefix projection on all fake platforms.
- Rollback restores a previous owned Docker symlink target on Linux, macOS, and Windows fake hosts.
- Rollback restores previous service enabled/running state on Linux, macOS, and Windows fake hosts.
- Reinstall removes stale activation records and stale owned Docker/PATH projections on Linux, macOS, and Windows fake hosts.

Run: `cargo test -p crosspack-cli service_activation_transaction_`

Expected: FAIL before wiring exists.

- [ ] **Step 2: Wire install-time service activation**

Only after Tasks 1-7 are green:

- During install, after prefix integration projection, inspect service integrations with `enable=true`.
- Plan service activation for the selected fake/real host context.
- Journal rollback before mutation.
- Apply activation.
- On failure, rollback and fail transaction.

- [ ] **Step 3: Wire uninstall cleanup**

Uninstall must stop/disable/remove activation state before removing prefix projections.

- Preserve foreign host paths.
- Remove only activation records owned by the uninstalling package identity.
- If cleanup cannot safely prove ownership, report a warning and leave the path intact.

- [ ] **Step 4: Run transaction tests**

Run: `cargo test -p crosspack-cli service_activation_transaction_`

Expected: PASS.

---

## Task 9: Registry Smoke Packages And Docs

**Files:**
- Modify: `registry/` package templates/manifests for smoke packages.
- Do not stage or commit the root repo's `registry` gitlink pointer; registry GitHub Actions propagate submodule updates back to the root repo.
- Modify shipped docs only if command behavior differs from existing docs.
- Test: registry metadata validation plus fake-executor install smoke tests.

- [ ] **Step 1: Snapshot registry submodule state before edits**

Run:

```bash
git submodule status registry
git -C registry status --short
```

Expected: registry submodule is initialized and any pre-existing registry changes are understood before editing. If the submodule has unrelated dirty changes, stop and ask how to isolate them.

- [ ] **Step 2: Add failing registry smoke tests**

Tests must cover package metadata for:

- Docker CLI plugin integration, e.g. `docker-compose` with `docker_cli_plugin:compose`.
- PATH plugin integration, e.g. `kubectx` with `path_plugin:kubectl:ctx`.
- Service integration with platform metadata for Linux, macOS, and Windows.

Add tests that parse registry package manifests/templates and assert:

```rust
assert!(manifest.integrations.iter().any(|integration| integration.kind() == "docker_cli_plugin"));
assert!(manifest.integrations.iter().any(|integration| integration.kind() == "path_plugin"));
assert!(manifest.integrations.iter().any(|integration| integration.kind() == "service"));
```

Run the focused registry validation command used by the registry crate for package metadata. If the exact test name differs after inspection, use the closest package manifest validation test and record it in the implementation notes.

Expected: FAIL before registry smoke package metadata exists.

- [ ] **Step 3: Add registry smoke package metadata**

Add or update registry package entries so smoke coverage includes:

- `docker-compose`: declares `[[integrations]] kind = "docker_cli_plugin"`, `name = "compose"`, and an artifact-relative `source`.
- `kubectx`: declares `[[integrations]] kind = "path_plugin"`, `host = "kubectl"`, `name = "ctx"`, and an artifact-relative `source`.
- A service package: declares `[[integrations]] kind = "service"`, `enable = true`, and platform-specific service metadata sources: `linux_systemd_user`, `macos_launch_agent`, and `windows_service`.

All sources must be normalized relative paths. Do not add scripts or lifecycle hooks.

- [ ] **Step 4: Add fake-executor registry install smoke tests**

Tests must install or plan-install the smoke packages against fake Linux, macOS, and Windows hosts and assert:

- Docker and PATH integrations project under the prefix without host mutation during install.
- Service package with `enable = true` plans platform-appropriate activation on Linux, macOS, and Windows.
- Service activation uses fake executors only.

Run:

```bash
cargo test -p crosspack-registry integration_smoke_
cargo test -p crosspack-cli service_activation_transaction_
```

Expected: PASS.

- [ ] **Step 5: Commit registry submodule changes without staging root pointer**

After registry tests pass, capture registry changes inside the submodule only. Do not run `git add registry` in the root repo.

```bash
git -C registry status --short
git -C registry diff --stat
git -C registry add <changed-registry-files>
git -C registry commit -m "test: add typed host integration smoke packages"
git -C registry status --short
git status --short
git submodule status registry
```

Expected:

- The registry submodule commit succeeds unless the user explicitly tells the implementer not to commit submodule changes.
- `git -C registry status --short` is clean after the submodule commit.
- Root `git status --short` may show ` M registry` because the worktree submodule checkout moved, but the root gitlink must not be staged.
- Root `git submodule status registry` may show a leading `+` until registry GitHub Actions propagate the pointer update back to the root repo.

Root repository commit is still not created unless the user explicitly asks for a root commit.

- [ ] **Step 6: Add in-repo docs if command behavior is shipped**

If command behavior becomes user-facing in README or architecture docs, update only the relevant in-repo docs. Do not update roadmap-only docs unless the roadmap changed.

- [ ] **Step 7: Run combined validation**

Run:

```bash
cargo test -p crosspack-core integration_service_platform_sources_
cargo test -p crosspack-cli integrations_
cargo test -p crosspack-installer activation_
cargo test -p crosspack-registry integration_smoke_
```

Expected: PASS.

---

## Final Verification

- [ ] Run `cargo fmt --all --check`
- [ ] Run `cargo test -p crosspack-installer activation_`
- [ ] Run `cargo test -p crosspack-cli integrations_`
- [ ] Run `cargo test -p crosspack-cli service_activation_transaction_`
- [ ] Run `cargo test -p crosspack-core integration_service_platform_sources_`
- [ ] Run `cargo test -p crosspack-registry integration_smoke_`
- [ ] Run `cargo test -p crosspack-cli`
- [ ] Run `cargo test -p crosspack-installer`
- [ ] Run `cargo build --workspace --locked`
- [ ] Run `cargo test --workspace`
- [ ] Run `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] Run `cargo clippy -p crosspack-cli --all-targets -- -D warnings`
- [ ] Run `cargo clippy -p crosspack-installer --all-targets -- -D warnings`
- [ ] Run `git -C registry status --short` and confirm it is clean
- [ ] Run `git submodule status registry` and confirm any leading `+` is expected from the submodule-only registry commit
- [ ] Run `git status --short` and confirm the root `registry` gitlink pointer is not staged for commit
- [ ] Generate critique diff URL with filters for changed files.

## Plan Self-Review

- Spec coverage: activation state, planner, Docker, PATH, services, CLI, transactions, platform metadata, registry smoke packages, and verification are covered.
- Platform coverage: Linux, macOS, and Windows are explicit in planner, adapter, transaction, and CLI tests.
- Safety: host mutation is isolated in installer adapters, planned before mutation, journaled before apply, and rollback-aware.
- Scope: install-time activation is only for service `enable=true`; Docker/PATH remain explicit commands.
- No placeholders: every task has concrete files, test names, commands, and expected results.
