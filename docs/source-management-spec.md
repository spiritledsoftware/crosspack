# Source Management and Metadata Update Specification (Draft v0.3)

This document defines the v0.3 source-management feature set for Crosspack. It introduces a multi-registry model with explicit trust pinning, snapshot updates, and deterministic source precedence.

## Scope

This spec covers:

- Registry source configuration in the user prefix.
- CLI commands for adding, listing, removing, and updating sources.
- Snapshot fetch and trust verification rules.
- How query and install commands read from multiple sources.
- Required tests and backward compatibility constraints.

This spec does not cover:

- HTTP mirror rotation or CDN policies.
- Distributed key revocation infrastructure.
- Package publish workflow.

## Goals

- Keep a Homebrew-like local-first workflow with simple commands.
- Add APT-like trust pinning and fail-closed metadata usage.
- Keep package selection deterministic across sources.
- Keep existing `search`, `info`, `install`, and `upgrade` behavior stable unless source configuration requires stricter validation.

## Non-Goals

- No interactive prompts.
- No automatic trust-on-first-use by default.
- No per-command ad hoc source ordering flags.

## Terminology

- Source: a registry origin (git URL or local filesystem path).
- Snapshot: a verified local copy of a source at a specific revision.
- Fingerprint: SHA-256 hex digest of the raw `registry.pub` bytes.
- Source precedence: deterministic ordering used when the same package exists in multiple sources.

## CLI Contract

### `crosspack registry add`

Add a new source record.

```text
crosspack registry add <name> <location> --kind <git|filesystem> --priority <u32> --fingerprint <64-hex>
```

Rules:

- `<name>` must match `^[a-z0-9][a-z0-9_-]{0,63}$`.
- `--priority` lower number means higher precedence.
- `--fingerprint` is required and must be exactly 64 lowercase or uppercase hex characters.
- Existing source name causes a hard error.
- Command validates format only; remote availability is validated by `crosspack update`.

Deterministic output:

- `added registry <name>`
- `kind: <kind>`
- `priority: <priority>`
- `fingerprint: <first16>...`

### `crosspack registry list`

List configured sources sorted by `(priority asc, name asc)`.

```text
crosspack registry list
```

Deterministic output per source:

- `<name> kind=<kind> priority=<priority> location=<location> snapshot=<snapshot-state>`

Snapshot state values:

- `none`
- `ready:<snapshot-id>`
- `error:<reason-code>`

### `crosspack registry remove`

Remove source configuration.

```text
crosspack registry remove <name> [--purge-cache]
```

Rules:

- Removing unknown source is a hard error.
- `--purge-cache` removes local snapshot cache for that source.
- Without `--purge-cache`, cached snapshot remains on disk but is ignored.

Deterministic output:

- `removed registry <name>`
- `cache: purged` or `cache: kept`

### `crosspack update`

Refresh snapshots from configured sources.

```text
crosspack update [--registry <name>]...
```

Rules:

- No `--registry` means update all configured sources.
- Repeating `--registry` narrows the target set.
- Unknown `--registry` name is a hard error.
- Exit code is non-zero if any targeted source fails update.

Per-source stable status values:

- `updated`
- `up-to-date`
- `failed`

Summary line:

- `update summary: updated=<n> up-to-date=<n> failed=<n>`

## State Layout

All source state is under the Crosspack prefix.

```text
<prefix>/state/registries/
  sources.toml
  cache/
    <name>/
      registry.pub
      index/
      snapshot.json
```

### `sources.toml`

```toml
version = 1

[[sources]]
name = "core"
kind = "git"
location = "https://example.test/crosspack-index.git"
priority = 100
fingerprint_sha256 = "4f9f2b7f52e6c9f4d0b77f3b9a0d6d8553cb4f4d2c5e8a4462f2386e2b1c0e17"
enabled = true
```

Rules:

- Serializer must emit sources sorted by `(priority, name)` for deterministic diffs.
- `enabled` defaults to `true` when missing.
- Unknown fields are ignored for forward compatibility.

### `snapshot.json`

```json
{
  "version": 1,
  "source": "core",
  "snapshot_id": "git:5f1b3d8a1f2a4d0e",
  "updated_at_unix": 1771000000,
  "manifest_count": 4123,
  "status": "ready"
}
```

Rules:

- `snapshot_id` format is `git:<short-commit>` for git sources and `fs:<sha256>` for filesystem sources.
- Snapshot file is written only after full verification succeeds.

## Update Pipeline

For each targeted source, `crosspack update` performs:

1. Sync source into a temporary directory.
2. Validate required files:
   - `registry.pub`
   - `index/`
3. Compute fingerprint from fetched `registry.pub` and compare against `sources.toml`.
4. Verify metadata signature policy can be enforced (sidecar files must be present for manifests that are read by registry APIs).
5. Atomically replace `<prefix>/state/registries/cache/<name>/`.
6. Write `snapshot.json`.

If any step fails, existing cache for that source remains unchanged.

## Metadata Read Model

`search`, `info`, `install`, and `upgrade` use only local verified snapshots under `state/registries/cache/`.

Rules:

- Commands never read directly from remote sources.
- If no verified snapshot exists for any enabled source, metadata-dependent commands fail.
- Each manifest still requires detached signature verification (`<version>.toml.sig`) and trusted key (`registry.pub`) in the source snapshot.

## Source Precedence and Package Selection

When the same package name exists in multiple sources:

1. Source with lowest numeric `priority` wins.
2. If priority ties, lexicographically smaller source name wins.
3. Lower-precedence sources are ignored for that package name.

Rationale:

- Avoid mixed-source version sets for a single package.
- Keep deterministic behavior and reduce trust-surface ambiguity.

## Error Semantics

Required error classes:

- `source-config-invalid`
- `source-not-found`
- `source-sync-failed`
- `source-key-fingerprint-mismatch`
- `source-snapshot-missing`
- `source-metadata-invalid`

Errors must include source name and actionable context.

## Backward Compatibility

- Existing single-root usage via `--registry_root` remains valid for development and tests.
- If `sources.toml` is absent, commands behave as follows:
  - with `--registry_root`: current behavior.
  - without `--registry_root`: fail with guidance to run `crosspack registry add` and `crosspack update`.
- Receipt format remains backward compatible in v0.3, but new optional fields may be added in later versions.

## Testing Requirements

### `crosspack-cli`

- Parse and validation tests for `registry add/list/remove/update` command shapes.
- Deterministic output ordering tests for `registry list`.
- Exit-code and summary tests for partial update failure.

### `crosspack-registry`

- Multi-source precedence tests for same package name.
- Strict fail when all enabled sources have no verified snapshot.
- Continue enforcing signature verification per manifest.

### `crosspack-security`

- Fingerprint generation tests against known vectors.
- Mismatch behavior tests for pinned fingerprint validation.

### Integration

- End-to-end test: add two sources with overlapping package names, update both, confirm precedence and install target source.

## Documentation Updates Required

- `docs/architecture.md`: add source-management module flow.
- `docs/registry-spec.md`: document source snapshot cache model.
- `docs/install-flow.md`: include precondition that dependency resolution reads verified snapshots.
