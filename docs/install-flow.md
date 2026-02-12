# Install Flow (v0.2)

`crosspack install <name[@constraint]>` executes this sequence:

1. Resolve package version from registry manifests.
2. Select artifact for requested target (`--target` or host triple).
3. Determine archive type (`artifact.archive` or infer from URL suffix).
4. Resolve cache path at:
   - `<prefix>/cache/artifacts/<name>/<version>/<target>/artifact.<ext>`
5. Download artifact if needed (or if `--force-redownload`).
6. Verify artifact SHA-256 against manifest `sha256`.
7. Extract archive into temporary state directory.
8. Apply `strip_components` during staging copy.
9. Move staged content into `<prefix>/pkgs/<name>/<version>/`.
10. Write install receipt to `<prefix>/state/installed/<name>.receipt`.

## Receipt Fields

- `name`
- `version`
- `target` (optional for backward compatibility)
- `artifact_url` (optional)
- `artifact_sha256` (optional)
- `cache_path` (optional)
- `install_status` (`installed`)
- `installed_at_unix`

## Failure Handling

- Checksum mismatch: cached artifact is removed and install fails.
- Unsupported archive type: install fails with actionable message.
- Extraction failure: temporary extraction directory is cleaned up best-effort.
- Incomplete download: `.part` file is removed on failed download.

## Current Limits

- Installs only the direct requested package (no dependency install yet).
- Does not create `bin` symlinks/shims yet.
- Upgrade does not yet resolve/install transitive dependencies.
- Pin constraints are simple per-package semver requirements stored as files.

## Upgrade and Pin

- `crosspack pin <name@constraint>` writes a pin at `<prefix>/state/pins/<name>.pin`.
- `crosspack install` and `crosspack upgrade` both enforce pin constraints during version selection.
- `crosspack upgrade <name[@constraint]>` upgrades one installed package if a newer compatible version exists.
- `crosspack upgrade` upgrades all installed packages, preserving target triples from receipts.
- If a package is already current (or only older/equal versions match constraints), upgrade reports it as up to date.
