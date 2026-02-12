# Crosspack Architecture (v0.2)

Crosspack is a native, cross-platform package manager with first-class Windows support. It does not wrap existing package managers.

## Modules

- `crosspack-cli`: user-facing commands and output.
- `crosspack-core`: shared domain models (manifest and artifact metadata).
- `crosspack-registry`: reads and searches the package index.
- `crosspack-resolver`: resolves version constraints against available manifests.
- `crosspack-installer`: prefix layout and install/uninstall filesystem mechanics.
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

## Planned Lifecycle

1. Search and inspect package metadata from a Git-backed index.
2. Resolve dependencies using semver constraints.
3. Download and verify artifacts. (implemented for direct package install)
4. Extract to versioned package paths. (implemented for `zip`, `tar.gz`, `tar.zst`)
5. Expose binaries through symlinks (Unix) or shims (Windows).
6. Record install state for upgrades and uninstalls.

## Current CLI Behavior

- `search` and `info` query the local registry index.
- Registry metadata is trusted only when signature verification succeeds with `registry.pub` at the registry root.
- Every version manifest requires a detached hex signature sidecar at `<version>.toml.sig`.
- Metadata-dependent commands fail closed on missing or invalid registry key/signature material.
- `install` resolves a transitive dependency graph with pin constraints, selects artifacts, downloads to cache, verifies SHA-256, extracts into `<prefix>/pkgs/<name>/<version>`, and writes install receipts.
- `install` exposes declared binaries into `<prefix>/bin/` (symlinks on Unix, `.cmd` shims on Windows) and hard-fails on collisions.
- `install` supports:
  - `--target <triple>` to override host target selection.
  - `--force-redownload` to bypass artifact cache.
- `pin` stores per-package version constraints in `<prefix>/state/pins/<name>.pin`.
- `upgrade` upgrades one package (`upgrade <name[@constraint]>`) or all installed root packages (`upgrade`) while honoring pins.
- Global `upgrade` runs one solve per target group derived from root receipts and rejects cross-target package-name overlap; current install state is package-name keyed.
- `install` and `upgrade` persist `install_reason` in receipts (`root` for explicit installs, `dependency` for transitive installs), while preserving existing root intent on upgrades.
- `uninstall` is dependency-aware: it blocks removal when remaining roots still require the package, reports blocking roots, removes requested packages, and auto-prunes orphan dependencies.
- `uninstall` prunes unreferenced artifact cache files for removed packages.
- `list` reads install receipts from `<prefix>/state/installed/`.

## Deferred Items

- Multi-profile uninstall policies beyond root/dependency receipts.
