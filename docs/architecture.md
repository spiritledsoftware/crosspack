# Crosspack Architecture (v0.2)

Crosspack is a native, cross-platform package manager with first-class Windows support. It does not wrap existing package managers.

## Modules

- `crosspack-cli`: user-facing commands and output.
- `crosspack-core`: shared domain models (manifest and artifact metadata).
- `crosspack-registry`: reads and searches the package index.
- `crosspack-resolver`: resolves version constraints against available manifests.
- `crosspack-installer`: prefix layout and install/uninstall filesystem mechanics.
- `crosspack-security`: checksum/signature verification helpers.

## Install Layout

Crosspack uses a scoped prefix:

- `<prefix>/pkgs/<name>/<version>/...`
- `<prefix>/bin/`
- `<prefix>/state/`
- `<prefix>/cache/`

Default user prefixes:

- macOS/Linux: `~/.crosspack`
- Windows: `%LOCALAPPDATA%\\Crosspack`

## Planned Lifecycle

1. Search and inspect package metadata from a Git-backed index.
2. Resolve dependencies using semver constraints.
3. Download and verify artifacts. (implemented for direct package install)
4. Extract to versioned package paths. (implemented for `zip`, `tar.gz`, `tar.zst`)
5. Expose binaries through symlinks (Unix) or shims (Windows).
6. Record install state for upgrades and uninstalls.

## Current CLI Behavior

- `search` and `info` query the local registry index.
- `install` resolves a package version and target artifact, downloads to cache, verifies SHA-256, extracts into `<prefix>/pkgs/<name>/<version>`, and writes an install receipt.
- `install` supports:
  - `--target <triple>` to override host target selection.
  - `--force-redownload` to bypass artifact cache.
- `pin` stores per-package version constraints in `<prefix>/state/pins/<name>.pin`.
- `upgrade` upgrades one package (`upgrade <name[@constraint]>`) or all installed packages (`upgrade`) while honoring pins.
- `uninstall` removes installed package content by receipt and is idempotent when package is not installed.
- `list` reads install receipts from `<prefix>/state/installed/`.

## Deferred Items

- Dependency graph install (currently direct package only).
- Binary exposure (`bin` symlinks/shims).
- Dependency-aware uninstall and cache pruning.
