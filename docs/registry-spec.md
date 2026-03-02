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
- If a source defines optional community metadata in `sources.toml`, verify the configured recipe catalog path and signature before snapshot acceptance.

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
- Optional community recipe metadata is signed and validated with the same source trust root (`registry.pub`) and fails closed on missing/invalid signatures or invalid catalog content.
- If the entire registry root content is compromised (including `registry.pub`), this model does not provide authenticity guarantees for that compromised root.

## Optional Community Recipe Metadata

- Source records may include an optional `community` block in `sources.toml`.
- `community.recipe_catalog_path` points to a relative `.toml` file within the source snapshot (for example: `community/recipes.toml`).
- The recipe catalog requires a detached signature at `<recipe_catalog_path>.sig` and must verify against the source `registry.pub` key.
- Catalog schema currently supports `version = 1` and `[[recipes]] package = "<name>"` entries.
- Recipe entries must be strictly sorted by package name and each package must exist under `index/<package>/`.

## Source Management Commands

- `crosspack registry add <name> <location> --kind <git|filesystem> --priority <u32> --fingerprint <64-hex>`
- `crosspack registry list`
- `crosspack registry remove <name> [--purge-cache]`
- `crosspack update [--registry <name>]...`

Metadata command fallback behavior:

- `--registry-root` provided: use that root directly (legacy mode).
- `--registry-root` omitted: require at least one configured source with a ready snapshot.
