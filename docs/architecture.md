# Crosspack Architecture (v0.3)

Crosspack is a native, cross-platform package manager with first-class Windows support. It does not wrap existing package managers.

## Modules

- `crosspack-cli`: user-facing commands and output.
- `crosspack-core`: shared domain models (manifest and artifact metadata).
- `crosspack-registry`: reads and searches the package index.
- `crosspack-resolver`: resolves version constraints against available manifests.
- `crosspack-installer`: prefix layout, install/uninstall filesystem mechanics, and transaction apply/rollback coordination.
- `crosspack-security`: checksum verification and registry metadata signature verification helpers.

## Install Layout

Crosspack uses a scoped prefix:

- `<prefix>/pkgs/<name>/<version>/...`
- `<prefix>/bin/`
- `<prefix>/state/`
- `<prefix>/cache/`

Default user prefixes:

- macOS/Linux: `~/.crosspack`
- Windows: `%LOCALAPPDATA%\\Crosspack`

## Lifecycle

1. Search and inspect package metadata from configured verified source snapshots, or from `--registry-root` when explicitly overridden.
2. Resolve dependencies using semver constraints.
3. Download and verify artifacts. (implemented for direct package install)
4. Extract to versioned package paths. (implemented for `zip`, `tar.gz`, `tar.zst`)
5. Expose binaries through symlinks (Unix) or shims (Windows).
6. Record install state for upgrades and uninstalls.

## Current CLI Behavior

- `search` and `info` query the local registry index.
- Metadata command backend selection is:
  - if `--registry-root` is set, read directly from that registry root (legacy single-root mode),
  - otherwise read from configured snapshots under `<prefix>/state/registries/cache/`.
- If `--registry-root` is not set and no configured source has a ready snapshot, metadata-dependent commands fail with guidance to run `crosspack registry add` and `crosspack update`.
- `registry add <name> <location> --kind <git|filesystem> --priority <u32> --fingerprint <64-hex>` adds a source record.
- `registry list` prints configured sources sorted by `(priority, name)` and includes snapshot state (`none`, `ready:<id>`, `error:<reason>`).
- `registry remove <name> [--purge-cache]` removes a source and optionally deletes its cached snapshot.
- `update [--registry <name>]...` refreshes all or selected sources and prints per-source status plus `update summary: updated=<n> up-to-date=<n> failed=<n>`.
- Registry metadata is trusted only when signature verification succeeds with `registry.pub` at the registry root, which acts as the local trust anchor for that registry snapshot or mirror.
- Every version manifest requires a detached hex signature sidecar at `<version>.toml.sig`.
- Metadata-dependent commands fail closed on missing or invalid registry key/signature material.
- This trust model does not defend against compromise of the entire registry root content itself (for example, if both manifests and `registry.pub` are replaced together).
- `install` resolves a transitive dependency graph with pin constraints, selects artifacts, downloads to cache, verifies SHA-256, extracts into `<prefix>/pkgs/<name>/<version>`, and writes install receipts.
- `install` exposes declared binaries into `<prefix>/bin/` (symlinks on Unix, `.cmd` shims on Windows) and hard-fails on collisions.
- `install` supports:
  - `--target <triple>` to override host target selection.
  - `--force-redownload` to bypass artifact cache.
- `pin` stores per-package version constraints in `<prefix>/state/pins/<name>.pin`.
- `upgrade` upgrades one package (`upgrade <name[@constraint]>`) or all installed root packages (`upgrade`) while honoring pins.
- Global `upgrade` runs one solve per target group derived from root receipts and rejects cross-target package-name overlap; current install state is package-name keyed.
- `install` and `upgrade` persist `install_reason` in receipts (`root` for explicit installs, `dependency` for transitive installs), while preserving existing root intent on upgrades.
- `install` and `upgrade` persist `exposed_completions` receipt entries for package-declared completion files exposed under `<prefix>/share/completions/packages/<shell>/`.
- `uninstall` is dependency-aware: it blocks removal when remaining roots still require the package, reports blocking roots, removes requested packages, and auto-prunes orphan dependencies.
- `uninstall` prunes unreferenced artifact cache files for removed packages.
- Transaction recovery commands are shipped and operational:
  - `rollback [txid]` replays rollback for eligible failed/incomplete transactions.
  - `repair` clears stale transaction markers and reconciles interrupted state.
  - `doctor` reports prefix paths and transaction health status.
- Successful multi-package install/upgrade receipts in one transaction share a single `snapshot_id` to preserve metadata provenance.
- `list` reads install receipts from `<prefix>/state/installed/`.
- `completions <bash|zsh|fish|powershell>` prints shell completion scripts for the canonical `crosspack` binary name and includes a loader block for package-declared completions.
- `init-shell [--shell <bash|zsh|fish|powershell>]` prints shell setup snippets for PATH + completion loading; without `--shell`, shell is auto-detected (with deterministic fallback).
- Install scripts attempt best-effort shell setup by generating completion files under `<prefix>/share/completions/` and upserting one managed profile block; failures warn and do not abort install.

## GA Scope Statement

This architecture document describes current shipped behavior unless explicitly marked otherwise.

Roadmap specs (v0.4/v0.5) are design targets and non-GA until merged and validated in current command behavior/tests.

## Deferred Items

- Multi-profile uninstall policies beyond root/dependency receipts.

## Milestone Specs (Roadmap, non-GA)

The next architecture milestones are specified in dedicated docs. These are design targets and are not fully implemented yet.

Planned (non-GA) additions include resolver provider/conflict/replacement phases and expanded transaction coordinator policies beyond current shipped rollback/repair behavior.

- Dependency policy (`provides`, `conflicts`, `replaces`) and provider resolution: `docs/dependency-policy-spec.md`.
- Transaction journal, rollback, and crash recovery: `docs/transaction-rollback-spec.md`.

## Project Resources

- Install and lifecycle behavior details: `docs/install-flow.md`
- Manifest schema and signing policy details: `docs/manifest-spec.md`
- Contributor onboarding and PR flow: `docs/contributor-playbook.md`
- Release readiness flow and snapshot post-merge validation: `docs/release-checklist.md`
