# Registry Specification (Draft v0.1)

Crosspack starts with a Git-backed static index.

## Directory Shape

```text
index/
  registry.pub
  <package-name>/
    <version>.toml
    <version>.toml.sig
```

Examples:

- `index/ripgrep/14.1.0.toml`
- `index/ripgrep/14.1.0.toml.sig`
- `index/fd/10.2.0.toml`

## Sync Strategy

- Clone or fetch/pull the index repository locally.
- Read manifests from local disk.
- Keep a cached snapshot for deterministic resolution.

## Version Discovery

- Package versions are discovered by listing TOML files in `index/<package>/`.
- Manifests are parsed and sorted by semantic version.

## Security Baseline

- Artifacts must include SHA-256 digests.
- Registry metadata signing is strict and enabled by default.
- The trusted public key is `registry.pub` at the registry root.
- Each manifest must have a detached signature sidecar at `<version>.toml.sig`.
- The sidecar format is hex-encoded detached signature bytes.
- Operations that rely on registry metadata fail closed on signature or key errors.
