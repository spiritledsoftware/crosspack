# Registry Specification (Draft v0.1)

Crosspack starts with a Git-backed static index.

## Directory Shape

```text
index/
  <package-name>/
    <version>.toml
```

Examples:

- `index/ripgrep/14.1.0.toml`
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
- Registry metadata signing is required before v0.1 release.
