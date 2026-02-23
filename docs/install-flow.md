# Install Flow (v0.3)

`crosspack install <name[@constraint]>` executes this sequence:

1. Select metadata backend, then verify registry metadata before resolution:
   - with `--registry-root`, read from that root directly (legacy single-root mode),
   - without `--registry-root`, read from configured snapshots under `<prefix>/state/registries/cache/`,
   - if no configured source has a ready snapshot, fail with guidance to run `crosspack registry add` and `crosspack update`,
   - trust `registry.pub` from the registry root,
   - require `<version>.toml.sig` detached sidecar for each manifest,
   - verify sidecar signatures from hex-encoded signature data.
2. Resolve package graph from registry manifests:
   - merge dependency constraints transitively,
   - apply pin constraints to root and transitive packages,
   - produce dependency-first install order.
3. Select artifact for each resolved package for requested target (`--target` or host triple).
4. Determine archive type (`artifact.archive` or infer from URL suffix).
5. For each resolved package, resolve cache path at:
   - `<prefix>/cache/artifacts/<name>/<version>/<target>/artifact.<ext>`
6. Download artifact if needed (or if `--force-redownload`).
7. Verify artifact SHA-256 against manifest `sha256`.
8. Extract archive into temporary state directory.
9. Apply `strip_components` during staging copy.
10. Move staged content into `<prefix>/pkgs/<name>/<version>/`.
11. Preflight binary exposure collisions against existing receipts and on-disk `<prefix>/bin` entries.
12. Expose declared binaries:
    - Unix: symlink `<prefix>/bin/<name>` to installed package path.
    - Windows: write `<prefix>/bin/<name>.cmd` shim to installed package path.
13. Remove stale previously-owned binaries no longer declared for that package.
14. Write install receipt to `<prefix>/state/installed/<name>.receipt`.
     - set `install_reason=root` for requested roots,
     - set `install_reason=dependency` for transitive-only packages,
     - preserve existing `install_reason=root` when upgrading already-rooted packages.

`upgrade` with no package argument runs one dependency solve per target group derived from installed root receipts.

## Receipt Fields

- `name`
- `version`
- `target` (optional for backward compatibility)
- `artifact_url` (optional)
- `artifact_sha256` (optional)
- `cache_path` (optional)
- `exposed_bin` (repeated, optional)
- `dependency` (repeated `name@version`, optional)
- `install_reason` (`root` or `dependency`; legacy receipts default to `root`)
- `install_status` (`installed`)
- `installed_at_unix`

## Failure Handling

- Checksum mismatch: cached artifact is removed and install fails.
- Registry key/signature validation failure: install/upgrade and other metadata-dependent operations fail closed.
- Unsupported archive type: install fails with actionable message.
- Extraction failure: temporary extraction directory is cleaned up best-effort.
- Incomplete download: `.part` file is removed on failed download.
- Binary collision: install fails if a requested binary is already owned by another package or exists unmanaged in `<prefix>/bin`.
- Global solve downgrade requirement during `upgrade`: operation fails with an explicit downgrade message and command hint.

## Uninstall Flow

`crosspack uninstall <name>` executes this sequence:

1. Read all receipts and build a dependency graph from receipt dependencies.
2. Compute reachability from all remaining root receipts.
3. If target package is still reachable from any remaining root, block uninstall and report sorted blocking roots.
4. Otherwise remove the requested package and prune orphaned dependency closure no longer reachable from any remaining root.
5. For all removed packages:
   - remove package directories and exposed binaries,
   - remove receipt files,
   - collect cache paths from receipts.
6. Remove cache files that are no longer referenced by any remaining receipt.
7. Return deterministic uninstall result including status, pruned dependency names, and blocking roots (if blocked).

## Current Limits

- Pin constraints are simple per-package semver requirements stored as files.

## Upgrade and Pin

- `crosspack pin <name@constraint>` writes a pin at `<prefix>/state/pins/<name>.pin`.
- `crosspack install` and `crosspack upgrade` both enforce pin constraints during version selection.
- `crosspack upgrade <name[@constraint]>` upgrades one installed package if a newer compatible version exists.
- `crosspack upgrade` upgrades all installed root packages with one solve per target group, preserving each group's target triple from receipts.
- `crosspack upgrade` fails if grouped solves would touch the same package name across different targets; with current package-name keyed state, use separate prefixes for cross-target installs.
- If a package is already current (or only older/equal versions match constraints), upgrade reports it as up to date.

## Shell Setup and Completions

- `crosspack completions <bash|zsh|fish|powershell>` prints completion scripts to stdout.
- Completion generation targets the canonical `crosspack` command name.
- `crosspack init-shell` remains PATH-only output and does not emit completion setup.
- Unix installer (`scripts/install.sh`) auto-detects shell from `$SHELL` (`bash`, `zsh`, or `fish`) and, by default:
  - writes completion scripts to `<prefix>/share/completions/crosspack.<shell>`,
  - creates or updates a single managed profile block in `~/.bashrc`, `~/.zshrc`, or `~/.config/fish/config.fish`,
  - ensures PATH setup and completion sourcing are idempotent.
- Windows installer (`scripts/install.ps1`) writes PowerShell completion script to `<prefix>\share\completions\crosspack.ps1` and updates `$PROFILE.CurrentUserCurrentHost` with one managed block for PATH + completion sourcing.
- Installer shell setup is best-effort: unsupported shells or profile write failures print warnings and manual commands, but installation still succeeds.
- Opt out of installer shell setup with:
  - Unix: `CROSSPACK_NO_SHELL_SETUP=1`
  - Windows: `-NoShellSetup`

## Forward-Looking Extensions

The current flow is the v0.3 baseline. Planned extensions are specified in:

- Dependency policy and replacement/provider behavior: `docs/dependency-policy-spec.md`.
- Transaction journal, rollback, and crash recovery behavior: `docs/transaction-rollback-spec.md`.
