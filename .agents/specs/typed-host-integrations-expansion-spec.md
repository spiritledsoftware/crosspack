# Typed Host Integrations Expansion Spec

**Status:** roadmap, non-GA
**Related plans:** `.agents/plans/2026-04-27-typed-host-integrations-design.md`
**Last updated:** 2026-05-06

## Problem

Crosspack has the first safe slice of typed host integrations: manifest metadata, managed projection under the Crosspack prefix, sidecar state, uninstall cleanup, and read-only `crosspack integrations list/status`. That proves the declarative model, but host-facing activation is still incomplete. macOS and Windows must be first-class implementation targets, not later ports of a Linux-only design.

Crosspack needs typed, previewable, reversible adapters for Docker CLI plugins, PATH-style plugins, and services across Linux, macOS, and Windows without allowing arbitrary install scripts.

## Goals

- Implement typed host activation for `docker_cli_plugin`, `path_plugin`, and `service` integrations.
- Treat Linux, macOS, and Windows as first-class targets in planning, state, status, rollback, and tests.
- Keep service activation explicit-only until durable service rollback and uninstall cleanup are implemented end-to-end.
- Keep Docker and PATH plugin activation explicit through `crosspack integrations enable` until install-time semantics are separately defined.
- Keep all integration effects previewable, reversible, idempotent, and owned by Crosspack state.
- Preserve deterministic uninstall and rollback cleanup.
- Keep plain/non-interactive output stable and machine-oriented.
- Support metadata-driven shell init snippets through `crosspack init-shell` without treating shell setup as an activation lifecycle.

## Non-Goals

- Do not run arbitrary package lifecycle scripts.
- Do not mutate host-owned locations for metadata-only installs.
- Do not require root/admin escalation for managed prefix projection or read-only status.
- Do not manage unrelated services not declared by installed packages.
- Do not implement system-level service activation in the first activation slice.
- Do not silently fall back from failed host activation to a successful install when the manifest requested install-time service activation.
- Do not add a `shell_hook` integration kind or execute shell init during install.

## Current State

- `[[integrations]]` supports `docker_cli_plugin`, `path_plugin`, and `service` metadata.
- Installer projects integration files under `share/integrations`.
- Integration sidecar state records prefix projections and supports cleanup.
- `crosspack integrations list` and `crosspack integrations status <package> <integration>` report projected state.
- `service.enable` exists in `crosspack-core`, but shipped install behavior rejects `enable = true` before host mutation. Docker and PATH integrations do not have `enable` fields.
- Existing service commands have partial native adapter plumbing, but the integration activation state model is not complete enough for cross-platform lifecycle management.
- Package shell init metadata is separate from `[[integrations]]`; generated snippets live under `share/shell/init/<shell>/` and load only from `crosspack init-shell`.

## Target Behavior

Host integration has two layers:

1. **Managed prefix projection** inside the Crosspack prefix.
2. **Host activation** through explicit typed adapters.

```text
manifest [[integrations]]
        |
        v
prefix projection state
        |
        v
adapter planner ----> host capability detection
        |
        v
transaction journal ----> adapter apply ----> activation sidecar state
        |
        v
rollback/uninstall cleanup
```

Activation must be reversible and represented in state. All adapters must plan before mutation, journal rollback payloads before mutation, and write activation state only after successful mutation.

## Manifest Semantics

Supported integration kinds remain:

```toml
[[integrations]]
kind = "docker_cli_plugin"
name = "compose"
source = "docker-compose"

[[integrations]]
kind = "path_plugin"
host = "kubectl"
name = "ctx"
source = "kubectl-ctx"

[[integrations]]
kind = "service"
name = "caddy"
source = "services/caddy.service"
```

Shell init metadata is package-level, not an integration:

```toml
[[shell_init]]
name = "starship"
binary = "starship"
strategy = "eval_stdout"
bash = ["init", "bash"]
zsh = ["init", "zsh"]
fish = ["init", "fish"]
powershell = ["init", "powershell"]
```

Service source metadata is platform-specific and must be explicit. A service integration may either point to one portable metadata file with platform sections, or use platform-qualified source fields once the schema grows. The first implementation should use the existing `source` field as the Linux systemd unit source and reject macOS/Windows activation for that package unless validated metadata for those platforms is present.

Future cross-platform service metadata shape:

```toml
[[integrations]]
kind = "service"
name = "demo"
linux_systemd_user = "services/demo.service"
macos_launch_agent = "services/com.example.demo.plist"
windows_service = "services/demo.windows-service.toml"
```

Until those platform-specific fields exist, macOS and Windows service activation must return `invalid-service-metadata` rather than inventing metadata from a Linux unit file.

Rules:

- `enable` remains service-only.
- `service.enable = true` remains reserved for future install-time service activation and currently fails closed before host mutation.
- Docker/PATH activation is explicit through `crosspack integrations enable <package> <integration>`.
- Docker/PATH install-time activation requires a future manifest field with clearer semantics, not overloading `enable`.
- Services default to `scope = user`. The state model must include scope so `scope = system` can be introduced later.
- Cross-platform service activation requires platform-appropriate service metadata. Linux systemd unit files, macOS LaunchAgent plists, and Windows service descriptors are not interchangeable.
- `[[shell_init]]` supports only `strategy = "eval_stdout"` initially. `binary` must match a declared artifact binary. Shell args are arrays, not raw scripts, and are projected as deterministic Crosspack-owned snippets for `init-shell`.

## Adapter Matrix

| Integration kind | Linux | macOS | Windows |
|---|---|---|---|
| `docker_cli_plugin` | Symlink into Docker CLI plugin discovery path | Symlink into Docker CLI plugin discovery path | Symlink into Docker CLI plugin discovery path when allowed; otherwise deterministic `unsupported-host`/`host-path-conflict`, not copy fallback |
| `path_plugin` | Symlink into Crosspack-owned bin/plugin exposure path | Symlink into Crosspack-owned bin/plugin exposure path | Shim or symlink through Crosspack-owned bin/plugin exposure path, matching existing Windows executable exposure patterns |
| `service` | User-level `systemd --user` adapter | User-level `launchctl`/LaunchAgent adapter | User-level Windows service adapter only where supported without admin; otherwise deterministic unsupported/escalation reason |

Crosspack must test every adapter through fake executors and fake host filesystems, even when the current CI host is not that platform.

## Docker CLI Plugin Activation

Activation command:

```text
crosspack integrations enable <package> <integration>
crosspack integrations disable <package> <integration>
crosspack integrations status <package> <integration>
```

Behavior:

- Resolve `<integration>` by full key or unambiguous short name.
- Plan destination from the platform Docker CLI plugin discovery path.
- Create a symlink from the host discovery path to the managed prefix projection.
- Refuse to overwrite an existing non-Crosspack-owned file/symlink.
- If the destination already points to the same Crosspack projection, report idempotent success.
- If the destination is owned by another Crosspack package, fail with `host-path-conflict`.
- Record activation state with adapter kind, destination path, symlink target, package identity, and rollback action.

Default host paths:

- Linux: `$DOCKER_CONFIG/cli-plugins` if `DOCKER_CONFIG` is set and absolute, otherwise `$HOME/.docker/cli-plugins`.
- macOS: `$DOCKER_CONFIG/cli-plugins` if `DOCKER_CONFIG` is set and absolute, otherwise `$HOME/.docker/cli-plugins`.
- Windows: `%DOCKER_CONFIG%\cli-plugins` if `DOCKER_CONFIG` is set and absolute, otherwise `%USERPROFILE%\.docker\cli-plugins`.

Path rules:

- Relative `DOCKER_CONFIG` values are rejected with `unsupported-host` because host mutation targets must be absolute.
- Missing home/user profile environment values return `unsupported-host`.
- Paths are normalized before conflict checks and persisted using the platform's native separators.
- Plain output must preserve native path spelling; tests must cover both `/` and `\` path outputs.

Windows symlink behavior:

- Try symlink only when supported by the host policy/runtime.
- If symlink creation requires privileges not available, fail with `escalation-required` or `unsupported-host`.
- Do not copy as fallback in this phase; rollback semantics should stay simple and exact.

## PATH Plugin Activation

PATH plugins represent host-command plugin discovery, such as `kubectl-ctx` for `kubectl ctx`.

Behavior:

- Resolve `<integration>` by full key or unambiguous short name.
- Expose through Crosspack-owned paths only.
- Prefer symlink on Unix-like hosts.
- On Windows, follow existing Crosspack executable/shim behavior rather than inventing a separate plugin exposure mechanism.
- Refuse host-owned directory mutation unless the destination is under Crosspack's prefix.
- Record activation state with adapter kind, exposed path, package identity, and rollback action.

Activation target:

- First slice: Crosspack-owned `bin` exposure for plugin executables.
- Future slice: host-specific discovery directories only when the host tool has a safe user-level plugin directory.

## Service Activation

Services are first-class lifecycle integrations. Current shipped behavior keeps activation explicit-only; `enable = true` fails closed before host mutation.

Service states:

- `declared`: manifest declared a service integration.
- `projected`: service metadata was copied under Crosspack prefix.
- `installed`: adapter installed service metadata into a user-level host location.
- `enabled`: adapter enabled the service to start automatically.
- `running`: adapter started the service and status confirms running.
- `failed`: adapter attempted action and failed.
- `unsupported`: platform or host capability cannot support the requested action.

Service activation modes:

- Install with `enable = false`: project metadata only; status reports `projected` and `reason=not-enabled`.
- Install with `enable = true`: fail closed before host mutation and do not persist activation state until durable service rollback/uninstall cleanup is complete.
- Future activation failure during install must fail the transaction and roll back any service files or host state created in that transaction.
- Explicit service commands (`start`, `stop`, `restart`, `status`) operate only on declared/projected Crosspack services.

Linux adapter:

- Scope: `user`.
- Install user service metadata under the user-level systemd unit directory.
- Run `systemctl --user daemon-reload` after install/remove.
- Enable/start/status/stop through `systemctl --user`.
- If `systemctl` is missing or user manager is unavailable, return deterministic reason codes.

macOS adapter:

- Scope: `user`.
- Install LaunchAgent plist under the user's LaunchAgents location.
- Use `launchctl bootstrap`, `bootout`, `enable`, `disable`, `kickstart`, and `print`/status-equivalent commands where available.
- Preserve deterministic fallback reason codes when launchd commands or user domain are unavailable.
- Service metadata must be a validated plist or generated from typed manifest fields in a later slice. Do not run package-provided scripts to create plists.
- A Linux `.service` source is not valid macOS service metadata. If no LaunchAgent plist or future typed service metadata is present, activation returns `invalid-service-metadata`.

Windows adapter:

- Scope: `user` for first slice.
- Prefer a user-level service strategy only when Windows supports the requested service without admin escalation.
- If admin is required for Windows Service Control Manager registration, report `escalation-required` rather than attempting elevation.
- Status parsing must support deterministic outcomes for `running`, `stopped`, `failed`, and `unsupported`.
- Do not shell out through arbitrary package scripts or PowerShell snippets from manifests.
- A Linux `.service` source and macOS plist are not valid Windows service metadata. If no Windows service descriptor or future typed service metadata is present, activation returns `invalid-service-metadata`.

## State Model

Projection state remains separate from activation state.

Activation state sidecar should store one record per integration with a host activation decision:

```text
version=1
activation=<package-state-key>\t<integration-key>\t<kind>\t<adapter>\t<scope>\t<desired-state>\t<applied-state>\t<host-path>\t<reason-code>
```

Fields:

- package identity/state key
- integration kind
- integration key
- adapter kind (`docker-cli`, `path-plugin-bin`, `systemd-user`, `launchd-user`, `windows-service-user`, `none`)
- scope (`user`, `system`, or `none`)
- desired state (`projected`, `enabled`, `running`, `disabled`)
- applied state (`projected`, `installed`, `enabled`, `running`, `stopped`, `failed`, `unsupported`)
- host path or native id
- last action status and reason code

Persistence rules:

- Successful mutations are persisted with their applied state and rollback identity.
- Failed explicit activation attempts are persisted with `failed` or `unsupported` only when no host mutation occurred or rollback completed successfully.
- Failed install-time service activation records are kept in the failed transaction journal and rollback snapshot, but the final installed package state must not claim the package is installed successfully.
- Read-only status may derive transient `unsupported` or `adapter-tool-missing` when no activation sidecar exists.
- `status` must not mutate activation state.

Reason codes:

- `ok`
- `not-enabled`
- `unsupported-host`
- `adapter-tool-missing`
- `host-path-conflict`
- `escalation-required`
- `native-command-failed`
- `invalid-service-metadata`
- `state-missing`
- `state-ambiguous`

## Transactions, Rollback, And Uninstall

All mutating activation flows must use installer transaction preflight/state paths.

Requirements:

- Plan all activation changes before first mutation.
- Detect conflicts before mutation where possible.
- Journal rollback payloads before host mutation.
- If activation fails during install, rollback all activation effects from that transaction before reporting failure.
- Uninstall disables/stops/removes host activation before removing prefix projection.
- Rollback restores previous activation state exactly: existing owned symlink target, service enabled/running state, or absence.
- Cleanup must never delete unrelated files or unrelated host services.

## CLI/UX Contracts

Plain output should remain deterministic and additive.

Read-only projection status:

```text
integration package=kubectx name=ctx key=path_plugin:kubectl:ctx kind=path_plugin state=projected adapter=none reason=not-enabled path=path-plugins/kubectl/kubectl-ctx
```

Activation status:

```text
integration package=docker-compose name=compose key=docker_cli_plugin:compose kind=docker_cli_plugin state=enabled adapter=docker-cli reason=ok path=/home/user/.docker/cli-plugins/docker-compose
service package=caddy name=caddy state=running adapter=systemd-user scope=user applied=true reason=ok
```

Failure status:

```text
integration package=docker-compose name=compose key=docker_cli_plugin:compose kind=docker_cli_plugin state=projected adapter=docker-cli reason=host-path-conflict path=/home/user/.docker/cli-plugins/docker-compose
service package=caddy name=caddy state=unsupported adapter=launchd-user scope=user applied=false reason=adapter-tool-missing
```

Commands:

```text
crosspack integrations list
crosspack integrations status <package> <integration>
crosspack integrations enable <package> <integration>
crosspack integrations disable <package> <integration>
crosspack services list
crosspack services status <package> <service>
crosspack services start <package> <service>
crosspack services stop <package> <service>
crosspack services restart <package> <service>
```

## Testing Requirements

Tests must not depend on the CI host platform. Platform behavior should be tested through injectable host capability and command-executor abstractions.

Core tests:

- Parse/validate each integration kind.
- Keep `enable` service-only.
- Reject unknown fields on Docker/PATH integrations.
- Reject unsafe source paths.

Planner tests:

- Docker destination paths for Linux, macOS, and Windows.
- PATH plugin activation plans for Linux, macOS, and Windows.
- Service activation plans for Linux systemd user, macOS launchd user, and Windows user-service fallback.
- Unsupported platform/tool states produce deterministic reason codes.

Adapter tests:

- Docker symlink create/remove/idempotence/conflict on Linux, macOS, and Windows fake filesystems.
- PATH plugin exposure create/remove/idempotence/conflict on Linux, macOS, and Windows fake filesystems.
- systemd user command sequence and status parsing.
- launchd user command sequence and status parsing.
- Windows service/user-service command sequence and status parsing.
- Escalation-required behavior on Windows when SCM registration would require admin.

Transaction tests:

- Linux, macOS, and Windows fake hosts: install with service `enable = true` fails closed before host mutation until service cleanup is durable.
- Linux, macOS, and Windows fake hosts: activation failure fails install and restores prior host state.
- Linux, macOS, and Windows fake hosts: rollback restores adapter state after failed activation.
- Linux, macOS, and Windows fake hosts: uninstall preserves service activation records when host cleanup cannot be verified.
- Linux, macOS, and Windows fake hosts: reinstall removes stale activation records and stale owned host projections.

CLI tests:

- `integrations list/status` includes full keys and activation state.
- `integrations enable/disable` is idempotent.
- Ambiguous short integration names require full keys.
- Plain output line shapes remain stable.
- Windows status output includes native `\` paths without corrupting key/value parsing.
- macOS launchd and Linux systemd failure outputs use the same stable key ordering as successful output.

Registry tests:

- Registry packages exercise Docker plugin, PATH plugin, and service metadata.
- Service packages do not set `enable = true` until install-time service activation is durable.

## Rollout Plan

1. Done: read-only integration listing/status over existing projection state.
2. Add activation state model and read/write APIs without host mutation.
3. Add adapter planner with fake host capability model for Linux, macOS, and Windows.
4. Add Docker CLI plugin symlink activation behind explicit `integrations enable/disable`.
5. Add PATH plugin activation through Crosspack-owned bin exposure behind explicit `integrations enable/disable`.
6. Add service activation state and user-scope adapter abstraction.
7. Implement Linux systemd user adapter with fake-executor tests.
8. Implement macOS launchd user adapter with fake-executor tests.
9. Implement Windows user-service/SCM-aware adapter with fake-executor tests and deterministic escalation handling.
10. Wire service `enable = true` into install transactions after all three platform adapters have test coverage.
11. Add rollback and uninstall coverage for every adapter effect.
12. Add registry smoke packages and docs after activation behavior is fully tested.

## Resolved Decisions

- Service integrations with `enable = true` currently fail closed before host mutation; activation remains explicit-only.
- `enable` remains service-only.
- Docker and PATH plugin activation remains explicit through `crosspack integrations enable/disable`.
- Service support starts with user-level services, with scope modeled for future system-level support.
- Docker plugin host projection uses symlinks, not copy fallback.
- macOS and Windows must be covered by planner, adapter, state, and fake-executor tests before install-time service activation is wired.
