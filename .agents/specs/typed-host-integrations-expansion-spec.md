# Typed Host Integrations Expansion Spec

**Status:** roadmap, non-GA
**Related plans:** `.agents/plans/2026-04-27-typed-host-integrations-design.md`
**Last updated:** 2026-04-29

## Problem

Crosspack now has the first slice of typed host integrations: manifest metadata, managed projection under the Crosspack prefix, sidecar state, and uninstall cleanup. That proves the declarative model, but it intentionally stops short of host-facing activation such as Docker plugin placement, service enable/start, and platform-specific status management.

## Goals

- Expand typed integrations without allowing arbitrary install scripts.
- Add opt-in host projection and service lifecycle adapters.
- Keep integration effects previewable, reversible, and owned by Crosspack state.
- Preserve deterministic uninstall and rollback cleanup.
- Build registry packages that exercise each integration kind.

## Non-Goals

- Do not run arbitrary package lifecycle scripts.
- Do not mutate host-owned locations without explicit user opt-in or policy.
- Do not require root/admin escalation for metadata-only installs.
- Do not manage unrelated services not declared by installed packages.

## Current State

- `[[integrations]]` supports `docker_cli_plugin`, `path_plugin`, and `service` metadata.
- Installer projects integration files under `share/integrations`.
- Integration sidecar state records projections and supports cleanup.
- Services are staged metadata only; Crosspack does not enable/start/stop services yet.

## Target Behavior

Host integration has two layers:

1. Managed projection inside the Crosspack prefix.
2. Optional host activation through typed adapters.

Examples:

- Docker CLI plugin: link or copy projected plugin into Docker's plugin discovery path when enabled.
- PATH plugin: expose plugin through Crosspack-managed bin or host-specific discovery path.
- Service: install/enable/start/stop/status through systemd user/system, launchd, or Windows Service Control Manager adapters.

Activation must be reversible and represented in state.

## Architecture

Typed integration adapters should be explicit modules rather than generic script runners.

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
adapter apply/rollback state
```

Responsibilities:
- `crosspack-core`: manifest schema and validation.
- `crosspack-installer`: projection state, host adapter state, uninstall cleanup, rollback payloads.
- `crosspack-cli`: enable/disable/status commands and output.
- Registry: packages with safe integration declarations.

## Data/State Model

Integration activation record:

- package identity
- integration kind
- integration key
- adapter kind
- desired state
- applied state
- host path or native id
- last action status and reason code

Service state should distinguish:

- declared
- projected
- installed
- enabled
- running
- failed
- unsupported

## CLI/UX Contracts

Potential command surface:

```text
crosspack integrations list
crosspack integrations enable <package> <integration>
crosspack integrations disable <package> <integration>
crosspack services list
crosspack services start <package> <service>
crosspack services stop <package> <service>
crosspack services status <package> <service>
```

Plain output should use stable key/value lines:

```text
integration name=docker-compose kind=docker_cli_plugin state=projected adapter=docker-cli reason=not-enabled
service package=caddy name=caddy state=stopped adapter=systemd-user applied=false reason=not-enabled
```

## Failure Modes

- Host adapter unavailable: report unsupported, keep prefix projection intact.
- Activation path conflict: fail before mutation unless owned by same package.
- Escalation required but not allowed: fail with deterministic reason.
- Host action fails: preserve state and report adapter reason code.
- Uninstall cleanup warning: continue managed cleanup only when safe and report warning.

## Testing Requirements

- Adapter planning tests for Linux, macOS, and Windows where platform code exists.
- Host path conflict tests.
- Enable/disable idempotence tests.
- Service status parsing tests with fake command executors.
- Rollback restores adapter state after failed activation.
- Uninstall removes host activation before removing prefix projection.
- Registry smoke packages for Docker plugin, PATH plugin, and service metadata.

## Rollout Plan

1. Add read-only integration listing/status over existing projection state.
2. Add adapter planning without host mutation.
3. Add opt-in Docker/PATH activation behind explicit commands.
4. Add service lifecycle adapters with fake-executor tests first.
5. Add rollback and uninstall coverage for adapter effects.

## Open Questions

- Should host activation happen during `install` when a package declares `enable=true`, or only through explicit commands?
- Should service support start with user-level services only?
- Should Docker plugin projection prefer symlink or copy semantics?
