# Registry Specification (Draft v0.3)

Crosspack uses configured registry sources with verified local snapshots.

## Directory Shape

```text
<prefix>/state/registries/
  sources.toml
  cache/
    <source-name>/
      registry.pub
      index/
        <package-name>/
          <version>.toml
          <version>.toml.sig
      snapshot.json
```

Legacy compatibility path when `--registry-root` is provided:

```text
<registry-root>/
  registry.pub
  index/
    <package-name>/
      <version>.toml
      <version>.toml.sig
```

## Sync Strategy

- Configure sources via `crosspack registry add`.
- Refresh snapshots via `crosspack update` (all sources by default, or selected via repeated `--registry <name>`).
- Read manifests from local verified snapshots on disk.
- Keep cached snapshots for deterministic resolution and source precedence.

## Version Discovery

- Package versions are discovered by listing TOML files in snapshot `index/<package>/`.
- Manifests are parsed and sorted by semantic version.
- If the same package exists in multiple sources, source precedence is deterministic: lowest priority wins, then lexical source name tie-break.

## Security Baseline

- Artifacts must include SHA-256 digests.
- Registry metadata signing is strict and enabled by default.
- `registry.pub` at the registry root is the local trust anchor for that registry snapshot or mirror.
- Each manifest must have a detached signature sidecar at `<version>.toml.sig`.
- The sidecar format is hex-encoded detached signature bytes.
- Operations that rely on registry metadata fail closed on signature or key errors.
- If the entire registry root content is compromised (including `registry.pub`), this model does not provide authenticity guarantees for that compromised root.

## Source Management Commands

- `crosspack registry add <name> <location> --kind <git|filesystem> --priority <u32> --fingerprint <64-hex>`
- `crosspack registry list`
- `crosspack registry remove <name> [--purge-cache]`
- `crosspack update [--registry <name>]...`

Metadata command fallback behavior:

- `--registry-root` provided: use that root directly (legacy mode).
- `--registry-root` omitted: require at least one configured source with a ready snapshot.
