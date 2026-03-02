# Manifest Specification (Draft v0.4)

Crosspack registry metadata is split across two TOML document types:

- package template document: `packages/<package>.toml`
- release document: `releases/<package>/<version>.toml`

At read time, Crosspack merges package + release data into runtime `PackageManifest` values.

## Package Template Document (`packages/<package>.toml`)

Package template docs store shared metadata and artifact templates.

### Required Fields

- `name`: package identifier
- `license`: package license string
- `homepage`: HTTPS homepage URL
- `source`: upstream release source metadata
- `artifacts`: non-empty array of artifact templates

### `source` Fields

- `provider`: currently `github`
- `repo`: `owner/name`
- `tag_prefix` (optional)
- `include_prereleases` (optional boolean)

### Artifact Template Fields (`[[artifacts]]`)

- `target`: Rust-style target triple
- `asset`: release asset-name template (usually includes `{version}`)
- `archive` (optional): extraction hint
- `strip_components` (optional): extraction hint
- `binaries`: required non-empty array of executable mappings
- `completions` (optional): shell completion mappings
- `gui_apps` (optional): GUI integration metadata

`asset` is template metadata only; resolved download URLs/checksums live in release docs.

## Release Document (`releases/<package>/<version>.toml`)

Release docs store version-specific resolved artifact data.

### Required Fields

- `name`: package identifier (must match `<package>` directory)
- `version`: semantic version (must match `<version>` filename)
- `artifacts`: non-empty array

### Release Artifact Fields (`[[artifacts]]`)

- `target`: Rust-style target triple
- `url`: HTTPS download URL
- `sha256`: expected SHA-256 digest of artifact bytes

## Runtime Manifest Fields (Merged Output)

After merge, Crosspack expects runtime manifest semantics equivalent to:

- `name`, `version`, `description` (optional), `license` (optional), `homepage` (optional)
- `dependencies` (optional map)
- `source_build` (optional)
- `services` (optional)
- `artifacts` with executable/completion/GUI metadata

The merge model allows package templates to carry stable metadata while release docs carry per-version URL/checksum data.

## Source Build Metadata (`source_build`)

`source_build` is parsed, validated, and used by source-build install flows.

- `url`: source archive or source tree URL
- `archive_sha256`: expected SHA-256 digest of downloaded source archive bytes
- `build_system`: build-system token (`cargo`, `cmake`, etc.)
- `build_commands`: deterministic command-token array (non-empty)
- `install_commands`: deterministic command-token array (non-empty)

## Service Declarations (`services`)

- `name`: service token exposed to `crosspack services`
- `native_id` (optional): host-native service identifier

Constraints:

- service tokens use package-token grammar (`[a-z0-9][a-z0-9._+-]{0,63}`)
- names must be unique per manifest
- invalid declarations fail closed

## Artifact Kind Policy

- Artifact ingestion is deterministic and fail-closed.
- Supported kinds: `zip`, `tar.gz`, `tar.zst`, `bin`, `msi`, `dmg`, `appimage`, `exe`, `pkg`, `msix`, `appx`.
- Pre-1.0 scope reset: `deb` and `rpm` are out of scope.
- Install mode defaults by kind:
  - managed: `zip`, `tar.gz`, `tar.zst`, `bin`, `dmg`, `appimage`
  - native: `pkg`, `exe`, `msi`, `msix`, `appx`

## Registry Metadata Signing

- Registry metadata signing is strict and enabled by default.
- Trusted key file is `registry.pub` at registry root.
- Every package and release TOML file must have a detached `.sig` sidecar.
- Sidecars are hex-encoded detached signature bytes.
- Metadata-dependent operations fail closed on missing/invalid key or signatures.

## Related Docs

- `docs/registry-spec.md`
- `docs/source-management-spec.md`
- `docs/install-flow.md`
