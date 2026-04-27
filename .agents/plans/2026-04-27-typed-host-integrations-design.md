# Typed Host Integrations Design

## Goal

Add a first slice of declarative host-integration metadata so Crosspack can model package-manager behavior beyond exposing binaries, while preserving transaction safety, previewability, uninstall cleanup, and the no-arbitrary-post-install-scripts boundary.

This slice should add packages that exercise three integration classes:

- `docker-compose`: Docker CLI plugin integration.
- `kubectx`: PATH-discovered kubectl plugin integration.
- `caddy`: service declaration/staged service metadata integration.

## Non-Goals

- Do not run arbitrary package lifecycle scripts.
- Do not run Docker, kubectl, systemd, launchd, or Windows service commands during install.
- Do not mutate user shell config or host app config files.
- Do not write to host-owned directories such as `~/.docker/cli-plugins` in the first slice.
- Do not change existing machine-oriented output line shapes unless required and covered by tests.

## Manifest Model

Add a manifest-level `[[integrations]]` array in `crosspack-core`.

Each integration is typed and validated. The first supported kinds are:

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
enable = false
```

Field meanings:

- `kind`: typed integration discriminator.
- `name`: integration-local name, validated with token grammar.
- `source`: relative path inside the installed package root.
- `host`: host command for `path_plugin`; first slice supports metadata and projection only.
- `enable`: service intent flag; first slice records intent but does not enable/start services.

The model is additive. Existing `services` remains accepted for compatibility, but new service metadata should prefer `[[integrations]] kind = "service"`.

## Storage Layout

All first-slice writes stay inside the Crosspack prefix:

```text
prefix/
  share/
    integrations/
      docker/cli-plugins/docker-compose
      path-plugins/kubectl/kubectl-ctx
      services/caddy/caddy.service
  state/
    installed/<package>.integrations
```

The installer copies integration sources into these managed projection paths. It records projected paths in a sidecar state file so reinstall and uninstall can remove stale projections.

Host projection into locations like `~/.docker/cli-plugins` is intentionally deferred. That later feature should be opt-in and conflict-aware.

## Installer Behavior

Install flow extends the existing exposure phase:

1. Install artifact into `pkgs/<name>/<version>` as today.
2. Expose declared binaries/completions/GUI assets as today.
3. Project declared integrations under `share/integrations`.
4. Remove stale integration projections from previous receipt sidecar state.
5. Write `<package>.integrations` sidecar state before/with the receipt.

Projection rules:

- `source` must be a normalized relative path with no `..`, root, or Windows prefix components.
- The resolved source must exist and be a file.
- Destination must be derived by Crosspack, not supplied as an arbitrary absolute path.
- Existing projected files owned by the same package can be replaced.
- Cross-package destination conflicts fail preflight.

## Uninstall Behavior

Uninstall reads `<package>.integrations` and removes recorded projection files. It prunes empty integration directories under `share/integrations` without deleting unrelated files.

The install receipt schema does not need to grow for the first slice; integration state can follow the existing `.gui` and `.services` sidecar pattern.

## Preview and Output

Dry-run preview should remain stable. If integration changes are surfaced, use additive change lines rather than altering existing tokens. The first implementation can keep preview unchanged and rely on install outcome detail lines for integration projections.

Install outcome can add a plain detail line:

```text
step integration_assets: docker_cli_plugin:docker/cli-plugins/docker-compose
```

Rich output should treat this as additive decoration only.

## Registry Package Coverage

Add package templates and release manifests for:

- `docker-compose` from `docker/compose` releases, exposing binary `docker-compose` and declaring `docker_cli_plugin`.
- `kubectx` from `ahmetb/kubectx` releases, exposing `kubectl-ctx` and declaring `path_plugin` for `kubectl ctx`.
- `caddy` from `caddyserver/caddy` releases, exposing `caddy` and declaring a disabled service integration staged from a packaged service template when available. If upstream artifacts do not include a unit file, add only the service integration metadata when a source file can be safely staged in registry metadata; otherwise choose a package whose release artifact includes service metadata.

## Testing

Core tests:

- Parse/validate each integration kind.
- Reject unknown integration kinds.
- Reject invalid tokens and unsafe source paths.

Installer tests:

- Project Docker CLI plugin into `share/integrations/docker/cli-plugins/docker-compose`.
- Project PATH plugin into `share/integrations/path-plugins/kubectl/kubectl-ctx`.
- Project service metadata into `share/integrations/services/<package>/<name>.service`.
- Remove stale integration projection on reinstall.
- Remove integration projection on uninstall.
- Fail preflight on cross-package projection conflict.

Registry tests:

- Validate `[[integrations]]` in package templates.
- Preserve generated package text for integrations.
- Smoke-install selected Linux artifacts for the three packages when practical.

## Risks

- Cross-platform package-manager parity can tempt arbitrary host mutation. This design avoids that by making Crosspack own typed implementations.
- Service support can become platform-specific quickly. This slice records/stages service metadata only; enable/start is future work.
- Docker plugin discovery will not work automatically until host projection is implemented. That is acceptable for this slice because it proves schema, projection, receipts, and cleanup safely first.

## Open Decisions

- If `caddy` release artifacts do not include a reusable service unit, pick another service-bearing package rather than inventing package-specific install scripts.
- Whether `services` should be migrated to `[[integrations]] kind = "service"` immediately or remain as a compatibility alias for one release cycle.
