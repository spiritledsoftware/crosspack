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

### Trust and Poison Taxonomy

Fatal source-level trust failures always fail closed for the source: missing `registry.pub`, configured fingerprint mismatch, missing ready `snapshot.json`, unreadable source layout, invalid cache replacement, or missing/invalid signatures for source metadata that exists in the snapshot.

Skippable package-level poison is limited to signed bytes whose provenance is trusted but whose package content cannot be used safely, such as TOML parse errors, manifest schema errors, missing required package fields, unsupported artifact structure, or a package already listed in durable quarantine state. Broad list/search/provider operations may skip these records and return diagnostics; direct selected package operations must fail when the selected package metadata is invalid.

Crosspack CLI warnings for skipped package-level metadata are additive stderr lines with quoted fields:

```text
warning: registry_package_skipped package="<name>" reason="package-metadata-invalid" source="<source>" detail="<detail>"
```

These warnings must not change existing machine-oriented install/update contracts, including `transaction_preview`, `transaction_summary`, `risk_flags`, `change_*`, or `update summary: updated=<n> up-to-date=<n> failed=<n>`.

### Automation Quarantine

Automation quarantine is advisory registry state stored under `state/upstream-release-bot.json`. It prevents repeated generated poison from blocking unrelated package updates. Source sync may accept ready snapshots that contain signed malformed package-level records, because signatures prove provenance even when package content is unusable. Clients may skip quarantined or malformed package-level records during broad list/search/provider operations, but they must still fail selected package operations when the selected package metadata is invalid.

The upstream release bot state uses schema v2 with top-level `schema_version`, `sources`, `packages`, and `quarantine` maps. `sources` records source cache/audit data by source identity; `packages` records package source identity/kind, latest seen version, last successful generated version, transient failure fields, and optional `backoff_until`; `quarantine` records `reason_code`, `detail`, first/last seen timestamps, attempted version, and optional last good version. Valid regenerated metadata plus package validation clears quarantine for that package.

The scheduled bot maintains one rolling PR from `upstream-release/rolling`. Each write run starts from current `main`, regenerates valid package updates, writes bot state, force-updates only that branch with `--force-with-lease`, and enables automerge. Unsigned generated TOML may appear in bot PRs before merge; the merge-time signing workflow owns `.toml.sig` sidecars.

Registry automation emits stable accounting lines for operators and CI logs:

```text
registry_update package=<name> status=quarantined reason=metadata-malformed attempted=<version>
registry_update package=<name> status=skipped reason=<rate-limited|upstream-error|backoff-active> reset_at=<iso8601>
registry_update_summary updated=<n> up_to_date=<n> quarantined=<n> transient_failed=<n> skipped=<n>
```

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
