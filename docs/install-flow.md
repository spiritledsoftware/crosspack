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
- `install_status` (`installed`)
- `installed_at_unix`

## Failure Handling

- Checksum mismatch: cached artifact is removed and install fails.
- Unsupported archive type: install fails with actionable message.
- Extraction failure: temporary extraction directory is cleaned up best-effort.
- Incomplete download: `.part` file is removed on failed download.
- Binary collision: install fails if a requested binary is already owned by another package or exists unmanaged in `<prefix>/bin`.
- Global solve downgrade requirement during `upgrade`: operation fails with an explicit downgrade message and command hint.

## Current Limits

- Pin constraints are simple per-package semver requirements stored as files.
- Dependency-aware uninstall and orphan cleanup are not implemented yet.

## Upgrade and Pin

- `crosspack pin <name@constraint>` writes a pin at `<prefix>/state/pins/<name>.pin`.
- `crosspack install` and `crosspack upgrade` both enforce pin constraints during version selection.
- `crosspack upgrade <name[@constraint]>` upgrades one installed package if a newer compatible version exists.
- `crosspack upgrade` upgrades all installed packages, preserving target triples from receipts.
- If a package is already current (or only older/equal versions match constraints), upgrade reports it as up to date.
