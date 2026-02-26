# Manifest Specification (Draft v0.2)

Each package version is represented by a TOML manifest stored in the registry index.

## Required Fields

- `name`: package identifier.
- `version`: semantic version.
- `artifacts`: list of downloadable package artifacts.

## Optional Fields

- `license`
- `homepage`
- `dependencies`: map of package name to semver constraint.

## Artifact Fields

- `target`: Rust-style target triple (for example `x86_64-pc-windows-msvc`).
- `url`: artifact download location.
- `sha256`: expected SHA-256 digest of artifact bytes.
- `size` (optional): expected size in bytes.
- `signature` (optional in v0.1): artifact-level detached signature reference.
- `archive` (optional): artifact kind override (`zip`, `tar.gz`, `tar.zst`, `bin`, `msi`, `dmg`, `appimage`, `exe`, `pkg`, `msix`, `appx`). If omitted, inferred from URL suffix (or inferred as `bin` for extensionless final URL path segments).
- `strip_components` (optional): number of leading path components to strip during extraction.
- `artifact_root` (optional): expected top-level extracted path (validation hint).
- `binaries` (optional): list of exposed commands for this artifact.
- `completions` (optional): list of shell completion files exposed for this artifact.
- `gui_apps` (optional): list of GUI application integrations exposed for this artifact.

### Artifact Kind Policy

- Artifact ingestion is deterministic and fail-closed.
- Pre-1.0 scope reset: `deb` and `rpm` are removed from the supported artifact-kind contract.
- Install mode defaults by artifact kind:
  - managed: `zip`, `tar.gz`, `tar.zst`, `bin`, `dmg`, `appimage`.
  - native: `pkg`, `exe`, `msi`, `msix`, `appx`.
- Host constraints are fail-closed:
  - Windows-only native kinds: `exe`, `msi`, `msix`, `appx`.
  - macOS-only native kind: `pkg`.
  - macOS-only managed kind: `dmg`.
  - Linux-only managed kind: `appimage`.
- Installer/package formats are deterministic and extraction-oriented; Crosspack does not run vendor installer UI/execution fallback flows.
- `appimage` artifacts are staged as direct payload files and require `strip_components = 0` with no `artifact_root` override.
- `bin` artifacts are staged as direct payload files using the downloaded file name and require `strip_components = 0` with no `artifact_root` override.
- `pkg` maintainer scripts are not executed; script-dependent installs fail closed.

### Native GUI Registration Policy

- GUI metadata may be projected into native platform registration locations.
- Native registration is best-effort and warning-driven (install success does not require adapter success).
- Known current limits:
  - Linux refresh depends on `update-desktop-database` availability.
  - Windows protocol/file-association registration is scoped to HKCU only.
  - macOS `.app` registration prefers bundle-copy deployment into `/Applications/<App>.app` and falls back to `~/Applications/<App>.app` when system destination prepare/write steps fail.
  - macOS registration refuses to overwrite unmanaged existing app bundles at either destination and emits warnings instead.
  - macOS LaunchServices refresh remains best-effort and warning-driven.

## Registry Metadata Signing

- Registry metadata signing is strict and enabled by default.
- The trusted registry key file is `registry.pub` at the registry root.
- Every manifest file `<version>.toml` must have a detached sidecar signature file `<version>.toml.sig`.
- Sidecar signatures are stored as hex-encoded detached signature bytes.
- Commands that rely on registry metadata fail closed on missing/invalid key or signature material.
- `artifact.signature` is separate from registry metadata sidecar signatures and applies only to downloaded artifacts.
- As of current GA behavior, registry metadata sidecar signatures are required and enforced; `artifact.signature` remains optional metadata and is not a prerequisite for install success.

## Planned Policy Extensions

The following schema and policy additions are planned but not part of the v0.2 baseline:

- v0.3 source management is an index/snapshot workflow change and does not add or modify manifest fields.

- v0.4 dependency policy fields (`provides`, `conflicts`, `replaces`): `docs/dependency-policy-spec.md`.
- Optional artifact signature enforcement policy details are planned for a future manifest/security spec update (non-GA; no enforcement in current release).

Until these milestones land, manifests should use the current v0.2 field set documented above.

## Related Docs

- Current install/runtime behavior: `docs/install-flow.md`
- Current architecture/module boundaries: `docs/architecture.md`
- Roadmap dependency policy (non-GA): `docs/dependency-policy-spec.md`
- Roadmap transaction policy (non-GA): `docs/transaction-rollback-spec.md`

### Artifact Binary Fields

- `name`: exposed command name placed into `<prefix>/bin/`.
- `path`: relative path inside extracted package content.

### Artifact Completion Fields

- `shell`: one of `bash`, `zsh`, `fish`, `powershell`.
- `path`: relative path inside extracted package content.

### Artifact GUI App Fields

- `app_id`: stable GUI application identifier (unique per artifact target).
- `display_name`: user-facing name for launcher metadata.
- `exec`: relative executable path inside extracted package content.
- `icon` (optional): relative icon path inside extracted package content.
- `categories` (optional): category labels for launcher metadata.
- `file_associations` (optional): list of file association declarations.
- `protocols` (optional): list of URL protocol handler declarations.

#### GUI File Association Fields

- `mime_type`: MIME type identifier.
- `extensions` (optional): list of file extensions (for example `.txt`, `.md`).

#### GUI Protocol Fields

- `scheme`: URL scheme token (for example `zed`, `myapp+internal`).

## Example

```toml
name = "ripgrep"
version = "14.1.0"
license = "MIT"
homepage = "https://github.com/BurntSushi/ripgrep"

[dependencies]
pcre2 = ">=10.0, <11.0"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://packages.example/ripgrep-14.1.0-x86_64-unknown-linux-gnu.tar.zst"
sha256 = "..."
strip_components = 1

[[artifacts.binaries]]
name = "rg"
path = "rg"

[[artifacts.gui_apps]]
app_id = "org.example.ripgrep-viewer"
display_name = "Ripgrep Viewer"
exec = "tools/rg-viewer"
categories = ["Utility", "Development"]

[[artifacts.gui_apps.file_associations]]
mime_type = "text/plain"
extensions = [".txt"]

[[artifacts.gui_apps.protocols]]
scheme = "rgview"

[[artifacts.completions]]
shell = "bash"
path = "completions/rg.bash"

[[artifacts.completions]]
shell = "zsh"
path = "completions/_rg"

[[artifacts]]
target = "x86_64-pc-windows-msvc"
url = "https://packages.example/ripgrep-14.1.0-x86_64-pc-windows-msvc.zip"
sha256 = "..."
archive = "zip"
artifact_root = "ripgrep-14.1.0-x86_64-pc-windows-msvc"

[[artifacts.binaries]]
name = "rg"
path = "rg.exe"
```
