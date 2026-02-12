# Crosspack Architecture (v0.1)

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
3. Download and verify artifacts.
4. Extract to versioned package paths.
5. Expose binaries through symlinks (Unix) or shims (Windows).
6. Record install state for upgrades and uninstalls.
