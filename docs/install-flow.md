# Install Flow (v0.2)

`crosspack install <name[@constraint]>` executes this sequence:

1. Resolve package graph from registry manifests:
   - merge dependency constraints transitively,
   - apply pin constraints to root and transitive packages,
   - produce dependency-first install order.
2. Select artifact for each resolved package for requested target (`--target` or host triple).
3. Determine archive type (`artifact.archive` or infer from URL suffix).
4. For each resolved package, resolve cache path at:
   - `<prefix>/cache/artifacts/<name>/<version>/<target>/artifact.<ext>`
5. Download artifact if needed (or if `--force-redownload`).
6. Verify artifact SHA-256 against manifest `sha256`.
7. Extract archive into temporary state directory.
8. Apply `strip_components` during staging copy.
9. Move staged content into `<prefix>/pkgs/<name>/<version>/`.
10. Preflight binary exposure collisions against existing receipts and on-disk `<prefix>/bin` entries.
11. Expose declared binaries:
   - Unix: symlink `<prefix>/bin/<name>` to installed package path.
   - Windows: write `<prefix>/bin/<name>.cmd` shim to installed package path.
12. Remove stale previously-owned binaries no longer declared for that package.
13. Write install receipt to `<prefix>/state/installed/<name>.receipt`.
    - set `install_reason=root` for requested roots,
    - set `install_reason=dependency` for transitive-only packages,
    - preserve existing `install_reason=root` when upgrading already-rooted packages.

`upgrade` with no package argument runs a single global dependency solve across all installed roots.

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
- `crosspack upgrade` upgrades all installed packages, preserving target triples from receipts.
- If a package is already current (or only older/equal versions match constraints), upgrade reports it as up to date.
