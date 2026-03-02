# Registry Specification (Draft v0.4)

Crosspack uses configured registry sources with verified local snapshots.

## Directory Shape

```text
<prefix>/state/registries/
  sources.toml
  cache/
    <source-name>/
      registry.pub
      packages/
        <package>.toml
        <package>.toml.sig
      releases/
        <package>/
          <version>.toml
          <version>.toml.sig
      snapshot.json
```

When `--registry-root` is provided, the pointed registry root must expose the same `registry.pub` + `packages/` + `releases/` contract.

## Sync Strategy

- Configure sources via `crosspack registry add`.
- Refresh snapshots via `crosspack update`.
- Read manifests from local verified snapshots only.
- Keep cached snapshots for deterministic resolution and source precedence.
- If a source defines optional community metadata in `sources.toml`, verify the configured recipe catalog path and signature before snapshot acceptance.

## Version Discovery and Merge Model

- Package names are discovered from `releases/<package>/` directories.
- Versions are discovered from `releases/<package>/<version>.toml` files.
- For every version lookup:
  1. verify `packages/<package>.toml(.sig)`,
  2. verify `releases/<package>/<version>.toml(.sig)`,
  3. merge package template + release document into runtime manifest data.
- If the same package exists in multiple sources, precedence is deterministic: lowest `priority` first, then lexical source name tie-break.

## Security Baseline

- Registry metadata signing is strict and enabled by default.
- `registry.pub` at the source root is the trust anchor.
- Both package and release TOML files require detached `.sig` sidecars.
- Sidecar format is hex-encoded detached signature bytes.
- Metadata-dependent operations fail closed on key or signature errors.
- Optional community recipe metadata is signed and validated against the same source trust root.

## Optional Community Recipe Metadata

- Source records may include an optional `community` block in `sources.toml`.
- `community.recipe_catalog_path` points to a relative `.toml` file within the source snapshot (for example: `community/recipes.toml`).
- The catalog requires a detached signature at `<recipe_catalog_path>.sig` and must verify against the source `registry.pub` key.
- Catalog schema supports `version = 1` and `[[recipes]] package = "<name>"` entries.
- Recipe entries must be strictly sorted by package name and each package must exist under `releases/<package>/`.

## Source Management Commands

- `crosspack registry add <name> <location> --kind <git|filesystem> --priority <u32> --fingerprint <64-hex>`
- `crosspack registry list`
- `crosspack registry remove <name> [--purge-cache]`
- `crosspack update [--registry <name>]...`
